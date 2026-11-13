<script setup lang="ts">
import ContentSkeleton from '../components/ContentSkeleton.vue';
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
    communityMcmod,
    communitySearch,
    communitySuggest,
    errText,
    getManifest,
    getModTargets,
    selectDir
} from '../api';
import { store, toast, selectedInstance, displayVersionName as versionLabel } from '../store';
import { instanceKey } from '@shared/modCompatibility';
import { communityFileMatchesInstance, usesCommunityLoader } from '@shared/communityPolicy';
import { mcmodSearchUrl, mcmodPageUrl } from '@shared/communityLinks';
import SelectMenu from '../components/SelectMenu.vue';
import UiGlyph from '../components/UiGlyph.vue';
import CommunityVersionFilter from '../components/CommunityVersionFilter.vue';
import { readCommunitySession, saveCommunitySession } from '../communitySession';
import {
    initialCommunityQuery,
    initialCommunityVersionSelection,
    chooseCommunityInstance,
    usableCommunityInstance,
    type CommunityVersionSource
} from '../communityVersionSelection';
const previousSession = readCommunitySession();
import MarqueeText from '../components/MarqueeText.vue';
import CommunityDownloadQueue from '../components/CommunityDownloadQueue.vue';
import { useCommunityDownloadQueue } from '../useCommunityDownloadQueue';
import CommunityFavorites from '../components/CommunityFavorites.vue';
import CommunityModDetails from '../components/CommunityModDetails.vue';
import { favorites, favoriteBusy, loadFavorites, toggleProject } from '../modFavorites';
onMounted(() => void loadFavorites());
import type {
    CommunityFile,
    CommunityKind,
    CommunityProjectReference,
    CommunityResult,
    CommunitySource,
    CommunitySuggestion,
    LoaderName
} from '@shared/types';
import type { InstalledVersion, RemoteVersion } from '@shared/types';

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

/**
 * 在启动器内打开网页（内联浏览器）。
 * 白名单之外或内联打开失败时，自动退回系统浏览器——玩家永远不会「点了没反应」。
 */
async function openInline(url: string, title: string) {
    try {
        await window.kamucl.invoke('inlineWeb:open', url, title);
    } catch {
        openExternal(url);
    }
}

/** MC 百科：词典/百科桥已附身份（mcmodId）直达 class 详情页（PCL 口径）；
 *  否则先实时精确检索，失败或无命中回退搜索页 */
async function openMcmod(item: CommunityProjectReference) {
    const direct = mcmodPageUrl({ ...item, slug: item.slug ?? '' });
    if (direct && item.mcmodId && item.mcmodId > 0) {
        await openInline(direct, item.title || 'MC 百科');
        return;
    }
    const searchUrl = mcmodSearchUrl({ ...item, slug: item.slug ?? '' });
    if (!searchUrl) {
        toast('该项目没有可用于检索的英文名称', 'error');
        return;
    }
    try {
        const keyword = new URL(searchUrl).searchParams.get('key') ?? '';
        const r = await communityMcmod(keyword);
        const hit = r.hits[0];
        if (hit?.url) {
            await openInline(hit.url, item.title || 'MC 百科');
            return;
        }
    } catch {
        // 百科查询失败：回退到搜索页
    }
    await openInline(searchUrl, item.title || 'MC 百科');
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
watch(communityTab, (tab) => {
    if (tab === 'browse') void nextTick(updateKindBlob);
});
function sectionKeyboard(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    communityTab.value =
        event.key === 'Home'
            ? 'browse'
            : event.key === 'End'
              ? 'favorites'
              : communityTab.value === 'browse'
                ? 'favorites'
                : 'browse';
    void nextTick(() =>
        document.querySelector<HTMLElement>(`[data-ui="community:${communityTab.value}"]`)?.focus()
    );
}

// ---------------- 搜索条件 ----------------
const PAGE_SIZE = 20;

const kindTabs: Array<{ value: CommunityKind; label: string }> = [
    { value: 'mod', label: 'Mod' },
    { value: 'modpack', label: '整合包' },
    { value: 'resourcepack', label: '资源包' },
    { value: 'shader', label: '光影包' },
    { value: 'datapack', label: '数据包' }
];

/** 类型筛选胶囊滑动指示块（与导航水滴/游戏 Tab 同款弹簧动效） */
const kindCapsules = ref<HTMLElement | null>(null);
const kindBlob = reactive({ left: 0, top: 0, width: 0, height: 0, on: false });
function updateKindBlob() {
    const root = kindCapsules.value;
    if (!root) return;
    const active = root.querySelector<HTMLElement>(`.capsule[data-kind="${query.kind}"]`);
    if (!active) return;
    kindBlob.left = active.offsetLeft;
    kindBlob.top = active.offsetTop;
    kindBlob.width = active.offsetWidth;
    kindBlob.height = active.offsetHeight;
    kindBlob.on = true;
}
// 字体、主题及换行会改变胶囊尺寸，分类切换以外也需要重算。
let kindBlobObserver: ResizeObserver | null = null;
onMounted(() => {
    nextTick(updateKindBlob);
    setTimeout(updateKindBlob, 200);
    kindBlobObserver = new ResizeObserver(() => updateKindBlob());
    watch(
        kindCapsules,
        (el) => {
            kindBlobObserver?.disconnect();
            if (el) kindBlobObserver?.observe(el);
        },
        { immediate: true }
    );
    // 主题切换改变配色/字体度量 → 重算
    watch(
        () => store.settings?.theme,
        () => nextTick(() => setTimeout(updateKindBlob, 60))
    );
});
onUnmounted(() => kindBlobObserver?.disconnect());
const kindBlobStyle = computed(() => ({
    left: kindBlob.left + 'px',
    top: kindBlob.top + 'px',
    width: kindBlob.width + 'px',
    height: kindBlob.height + 'px',
    opacity: kindBlob.on ? 1 : 0
}));

const sourceOptions: Array<{ value: 'all' | CommunitySource; label: string }> = [
    { value: 'all', label: '全部来源' },
    { value: 'modrinth', label: 'Modrinth' },
    { value: 'curseforge', label: 'CurseForge' }
];

const loaderOptions: Array<{ value: '' | LoaderName; label: string }> = [
    { value: '', label: '全部加载器' },
    { value: 'forge', label: 'Forge' },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'neoforge', label: 'NeoForge' }
];

/** 完整 MC 版本列表（与游戏下载页同一数据源：远程版本清单，正式版为主） */
const manifestVersions = ref<Array<{ id: string; type: RemoteVersion['type'] }>>([]);
const manifestLoading = ref(false);

async function loadManifest() {
    if (manifestVersions.value.length || manifestLoading.value) return;
    manifestLoading.value = true;
    try {
        const list = await getManifest();
        /*
         * 排序口径（PCL2 同款直觉）：正式版在前，按新到旧。
         * Mojang 清单把 26.3-rc-3 / 26.3-pre-3 这类预发布也标成 snapshot，
         * 只按 type 排会让它们与正式版交错（26.3 → 26.3-rc-3 → 26.3-rc-2 …），
         * 读起来像一堆同名版本。所以再按 id 后缀细分一档：预发布排在所有正式版之后。
         */
        const preRelease = /-(?:rc|pre)\d*$/i;
        const rank = (v: RemoteVersion) => {
            if (v.type === 'release') return 0;
            if (v.type === 'snapshot') return preRelease.test(v.id) ? 2 : 1;
            if (v.type === 'old_beta') return 3;
            return 4;
        };
        manifestVersions.value = [...list]
            .sort((a, b) => {
                const r = rank(a) - rank(b);
                if (r) return r;
                return a.releaseTime < b.releaseTime ? 1 : -1;
            })
            .map((v) => ({ id: v.id, type: v.type }));
    } catch {
        /* 清单失败时回退到已安装版本 */
        const set = new Set<string>();
        for (const v of store.installed) if (v.mcVersion) set.add(v.mcVersion);
        manifestVersions.value = [...set]
            .sort()
            .reverse()
            .map((id) => ({ id, type: 'release' as const }));
    } finally {
        manifestLoading.value = false;
    }
}

const versionInput = ref(previousSession?.versionInput ?? '');

onMounted(() => void loadManifest());

/** 消费资源页的语境交接（模组/资源包/光影包页「下载X…」跳入）：预填资源类型与目标实例 */
function consumeCommunityHandoff() {
    const handoff = store.communityHandoff;
    if (!handoff) return;
    store.communityHandoff = null;
    query.kind = handoff.kind;
    const inst = store.installed.find(
        (v) => v.id === handoff.versionId && usableCommunityInstance(v)
    );
    if (!inst) return;
    versionSelection.source = 'installed';
    versionSelection.instance = instanceKey(inst);
    versionSelection.loaderSource = 'instance';
    query.mcVersion = inst.mcVersion || '';
    query.loader = inst.loader || '';
    versionInput.value = inst.mcVersion || '';
}
watch(
    () => store.installed.length,
    () => consumeCommunityHandoff()
);
onMounted(() => {
    consumeCommunityHandoff();
    void loadManifest();
});

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
const loaderSummary = computed(() =>
    query.loader
        ? (loaderOptions.find((v) => v.value === query.loader)?.label ?? query.loader)
        : '全部加载器'
);
const manualLoaderMismatch = computed(
    () =>
        versionSelection.source === 'installed' &&
        versionSelection.loaderSource === 'manual' &&
        filterInstance.value &&
        (filterInstance.value.loader ?? '') !== query.loader
);
// query 必须先初始化。过早运行 getter 会抛错，导致监听未订阅分类变化。
watch(
    () => query.kind,
    () => nextTick(updateKindBlob),
    { flush: 'post' }
);
const supportsLoader = computed(() => usesCommunityLoader(query.kind));
const usesPagination = computed(() => !supportsLoader.value);

/** 排序选项（updated 映射：Modrinth index=updated / CurseForge sortField=3，HMCL 同款） */
const sortOptions = [
    { value: 'relevance', label: '相关度' },
    { value: 'downloads', label: '最多下载' },
    { value: 'newest', label: '最新发布' },
    { value: 'updated', label: '最近更新' }
];

/**
 * 分类标签（PCL PageDownloadMod.xaml 的 27 分类表，Tag 格式 "CF categoryId/Modrinth slug"：
 * CF 侧用 categoryId，Modrinth 侧用 slug；一侧为空则只发给另一侧）
 */
const categoryOptions: Array<{ value: string; label: string }> = [
    { value: '', label: '全部分类' },
    { value: '406/worldgen', label: '世界元素' },
    { value: '407/', label: '生物群系' },
    { value: '410/', label: '维度' },
    { value: '408/', label: '矿物与资源' },
    { value: '409/', label: '天然结构' },
    { value: '412/technology', label: '科技' },
    { value: '415/', label: '管道与物流' },
    { value: '4843/', label: '自动化' },
    { value: '417/', label: '能源' },
    { value: '4558/', label: '红石' },
    { value: '436/food', label: '食物与烹饪' },
    { value: '416/', label: '农业' },
    { value: '/game-mechanics', label: '游戏机制' },
    { value: '414/transportation', label: '运输' },
    { value: '420/storage', label: '仓储' },
    { value: '419/magic', label: '魔法' },
    { value: '422/adventure', label: '冒险' },
    { value: '424/decoration', label: '装饰' },
    { value: '411/mobs', label: '生物' },
    { value: '5191/utility', label: '实用' },
    { value: '434/equipment', label: '装备与工具' },
    { value: '9026/', label: '创造模式' },
    { value: '6814/optimization', label: '性能优化' },
    { value: '423/', label: '信息显示' },
    { value: '435/social', label: '服务器' },
    { value: '421/library', label: '支持库' }
];

// ---------------- 搜索与列表 ----------------
const results = ref<CommunityResult[]>(previousSession?.results ?? []);
const loading = ref(false);
/** 结果切换淡入标记（无感刷新：旧结果在位，新结果轻淡入） */
const swapAnim = ref('');
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
const listCard = ref<HTMLElement | null>(null);

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
            category: query.category || undefined,
            sort: query.sort,
            offset: requestedOffset,
            limit: PAGE_SIZE
        });
        if (generation !== searchGeneration) return;
        const list = response.items;
        if (first) {
            // 结果有变化才淡入；同结果重搜不闪
            const prevKey = results.value.map(itemKey).join('|');
            const nextKey = list.map(itemKey).join('|');
            if (prevKey !== nextKey && results.value.length) {
                swapAnim.value = false;
                void nextTick(() => {
                    swapAnim.value = true;
                    setTimeout(() => (swapAnim.value = ''), 240);
                });
            } else if (!results.value.length) {
                swapAnim.value = true;
                setTimeout(() => (swapAnim.value = ''), 240);
            }
            results.value = list;
        } else results.value = [...results.value, ...list];
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

// ---------------- 输入联想（离线词典 LCS + 内置别名，纯本地零网络） ----------------
const suggestions = ref<CommunitySuggestion[]>([]);
const suggestOpen = ref(false);
const suggestActive = ref(-1);
let suggestTimer: ReturnType<typeof setTimeout> | undefined;
let suggestSeq = 0;

function closeSuggest() {
    suggestOpen.value = false;
    suggestActive.value = -1;
}

function onKeywordInput() {
    // 输入变化：先收起旧下拉，200ms 防抖后本地查联想（美西螈 use-browse-search 同款节流）
    closeSuggest();
    const keyword = query.keyword.trim();
    if (suggestTimer) clearTimeout(suggestTimer);
    if (keyword.length < 1 || query.kind !== 'mod') {
        suggestions.value = [];
        return;
    }
    const seq = ++suggestSeq;
    suggestTimer = setTimeout(async () => {
        try {
            const r = await communitySuggest(keyword, query.kind);
            if (seq !== suggestSeq) return;
            suggestions.value = r.suggestions;
            suggestOpen.value = r.suggestions.length > 0;
            suggestActive.value = -1;
        } catch {
            suggestions.value = [];
        }
    }, 200);
}

function pickSuggestion(s: CommunitySuggestion) {
    query.keyword = s.sublabel ? s.sublabel.replace(/\s*\([^)]*\)\s*$/, '') : s.label;
    suggestions.value = [];
    suggestOpen.value = false;
    suggestActive.value = -1;
    void doSearch(true);
}

function onKeywordKeydown(e: KeyboardEvent) {
    if (!suggestOpen.value || !suggestions.value.length) return;
    if (e.key === 'ArrowDown') {
        e.preventDefault();
        suggestActive.value = (suggestActive.value + 1) % suggestions.value.length;
    } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        suggestActive.value =
            (suggestActive.value - 1 + suggestions.value.length) % suggestions.value.length;
    } else if (e.key === 'Escape') {
        closeSuggest();
    }
}

/** 输入即搜：中文关键词停止输入 400ms 后自动搜索（英文仍走回车，避免打爆远端配额） */
let liveTimer: ReturnType<typeof setTimeout> | undefined;
function onKeywordSettled() {
    const keyword = query.keyword.trim();
    if (liveTimer) clearTimeout(liveTimer);
    if (!keyword || !/[\u3400-\u4dbf\u4e00-\u9fff]/u.test(keyword)) return;
    liveTimer = setTimeout(() => void doSearch(true), 400);
}
async function goToPage(page: number) {
    if (loading.value || page < 1 || page > totalPages.value || page === currentPage.value) return;
    await doSearch(false, page);
    listCard.value?.scrollIntoView({ block: 'start', behavior: 'smooth' });
}

// ---------------- 无限滚动：列表底部哨兵进入视口即自动加载（保留按钮作兜底） ----------------
const moreSentinel = ref<HTMLElement | null>(null);
let moreObserver: IntersectionObserver | null = null;
onMounted(() => {
    moreObserver = new IntersectionObserver(
        (entries) => {
            if (
                communityTab.value === 'browse' &&
                !usesPagination.value &&
                entries.some((e) => e.isIntersecting) &&
                hasMore.value &&
                !loading.value &&
                !loadingMore.value
            ) {
                onLoadMore();
            }
        },
        { root: null, rootMargin: '240px', threshold: 0 }
    );
    watch(
        moreSentinel,
        (el) => {
            moreObserver?.disconnect();
            if (el) moreObserver?.observe(el);
        },
        { immediate: true }
    );
});
onUnmounted(() => {
    moreObserver?.disconnect();
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
    query.category = '';
    query.sort = 'relevance';
    versionInput.value = '';
    closeSuggest();
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

// ---------------- 列表展示 ----------------
/** 图标加载失败的项目（显示首字母占位） */
const brokenIcons = ref(new Set<string>());
const itemKey = (r: CommunityProjectReference) => `${r.source}:${r.projectId}`;
const onIconError = (r: CommunityResult) => {
    brokenIcons.value = new Set([...brokenIcons.value, itemKey(r)]);
};

const fmtDownloads = (n: number): string => {
    if (n >= 1e8) return (n / 1e8).toFixed(1) + ' 亿';
    if (n >= 1e4) return (n / 1e4).toFixed(1) + ' 万';
    return String(n);
};

const fmtDate = (iso: string): string => {
    if (!iso) return '—';
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString('zh-CN');
};

const fmtSize = (bytes: number): string => {
    if (!bytes || bytes <= 0) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
};

const releaseTagClass = (t: CommunityFile['releaseType']) =>
    t === 'beta' ? 'tag-cyan' : t === 'alpha' ? 'tag-danger' : 'tag-gold';
const releaseText: Record<CommunityFile['releaseType'], string> = {
    release: '正式版',
    beta: 'Beta',
    alpha: 'Alpha'
};

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
    loader: '' as LoaderName | '',
    /** 外部目录（PCL 同款）：设置后资源直落该文件夹，不走实例 */
    externalDir: ''
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
        (isModpack.value ||
            !!modal.externalDir ||
            targetOptions.value.some((v) => instanceKey(v) === modal.versionId))
);

/** 选外部目录（app:selectDir），选中后清实例选择 */
async function pickExternalDir() {
    try {
        const dir = await selectDir();
        if (!dir) return;
        modal.externalDir = dir;
        modal.versionId = '';
    } catch {
        /* 取消选择 */
    }
}

function confirmDownload() {
    const file = selectedFile.value;
    if (!file || !canConfirm.value) return;
    const external = !isModpack.value && modal.externalDir ? modal.externalDir : '';
    const target = isModpack.value
        ? undefined
        : external
          ? undefined
          : targetOptions.value.find((v) => instanceKey(v) === modal.versionId);
    try {
        const folder =
            target?.folder ||
            store.settings?.folders.find((folder) => folder.isDefault)?.path ||
            store.settings?.gameDir ||
            '';
        const { added } = downloadQueue.enqueue({
            file,
            kind: modal.kind,
            target,
            folder,
            externalDir: external || undefined
        });
        modal.open = false;
        fileGeneration++;
        openGeneration++;
        toast(
            added
                ? modal.kind === 'mod'
                    ? '已加入社区队列，检测完成后请在队列确认前置与安装'
                    : external
                      ? '已加入社区下载队列，将保存到所选文件夹'
                      : '已加入社区下载队列，可继续浏览资源'
                : '该文件已在目标实例的社区队列中',
            'info'
        );
    } catch (e) {
        toast('加入队列失败：' + errText(e), 'error');
    }
}
</script>

<template>
    <div
        data-ui="CommunityView:6e56fae5e7dd"
        class="page page--content community-page"
        :data-design-page="query.kind"
    >
        <!-- 标题 -->
        <div data-ui="CommunityView:9346ef73457d" class="page-head">
            <h1 data-ui="CommunityView:5476a5545de6" class="page-title">社区资源</h1>
            <p data-ui="CommunityView:8ea54e89551b" class="page-sub">
                搜索并下载 Modrinth / CurseForge 上的 Mod、整合包、资源包、光影与数据包
            </p>
        </div>
        <CommunityDownloadQueue />

        <div
            class="community-sections"
            role="tablist"
            aria-label="社区资源分区"
            @keydown="sectionKeyboard"
        >
            <button
                class="community-section"
                role="tab"
                data-ui="community:browse"
                :tabindex="communityTab === 'browse' ? 0 : -1"
                :aria-selected="communityTab === 'browse'"
                :class="{ active: communityTab === 'browse' }"
                @click="communityTab = 'browse'"
            >
                找资源</button
            ><button
                class="community-section"
                role="tab"
                data-ui="community:favorites"
                :tabindex="communityTab === 'favorites' ? 0 : -1"
                :aria-selected="communityTab === 'favorites'"
                :class="{ active: communityTab === 'favorites' }"
                @click="communityTab = 'favorites'"
            >
                已收藏 MOD <span>{{ favorites.length }}</span>
            </button>
        </div>

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
            <!-- 搜索卡片 -->
            <div data-ui="CommunityView:5551ade589f2" class="card search-card">
                <div data-ui="CommunityView:cc7ad8d814fc" class="kind-capsules" ref="kindCapsules">
                    <span
                        data-ui="CommunityView:911176084f18"
                        class="capsule-blob"
                        :style="kindBlobStyle"
                        aria-hidden="true"
                    ></span>
                    <button
                        data-ui="CommunityView:9275c1ee8bbc"
                        v-for="t in kindTabs"
                        :key="t.value"
                        class="capsule"
                        :data-kind="t.value"
                        :class="{ active: query.kind === t.value }"
                        @click="
                            query.kind = t.value;
                            onFilterChange();
                        "
                    >
                        {{ t.label }}
                    </button>
                </div>

                <section class="version-filter-panel" aria-label="Minecraft 版本筛选">
                    <div class="version-filter-heading">
                        <strong>适用游戏版本</strong
                        ><span class="muted"
                            >{{ versionSummary
                            }}<template v-if="supportsLoader">
                                · {{ loaderSummary }}</template
                            ></span
                        >
                    </div>
                    <div class="version-source-options" role="group" aria-label="版本选择来源">
                        <button
                            type="button"
                            class="version-source-option"
                            data-ui="community:versions-all"
                            :class="{ active: versionSelection.source === 'all' }"
                            :aria-pressed="versionSelection.source === 'all'"
                            @click="chooseVersionSource('all')"
                        >
                            全部版本
                        </button>
                        <button
                            type="button"
                            class="version-source-option"
                            data-ui="community:versions-installed"
                            :class="{ active: versionSelection.source === 'installed' }"
                            :aria-pressed="versionSelection.source === 'installed'"
                            @click="chooseVersionSource('installed')"
                        >
                            已安装版本<span>{{ installedVersionOptions.length }}</span>
                        </button>
                        <button
                            type="button"
                            class="version-source-option"
                            data-ui="community:versions-custom"
                            :class="{ active: versionSelection.source === 'custom' }"
                            :aria-pressed="versionSelection.source === 'custom'"
                            @click="chooseVersionSource('custom')"
                        >
                            自定义版本
                        </button>
                    </div>
                    <div
                        v-if="versionSelection.source === 'installed'"
                        class="version-filter-detail"
                        data-ui="CommunityView:f7acd66aeb10"
                    >
                        <SelectMenu
                            v-if="installedVersionOptions.length"
                            class="instance-filter"
                            aria-label="选择已安装版本"
                            :model-value="versionSelection.instance"
                            placeholder="选择已安装实例以筛选…"
                            :options="installedVersionOptions"
                            @change="useInstance"
                        />
                        <p v-else class="muted">暂无可识别的已安装版本；可使用自定义版本。</p>
                        <button
                            data-ui="CommunityView:49df26abb0c5"
                            class="btn btn-ghost btn-sm"
                            :disabled="
                                !currentInstance || !usableCommunityInstance(currentInstance)
                            "
                            @click="useCurrentInstance"
                        >
                            使用当前实例
                        </button>
                    </div>
                    <div
                        v-if="versionSelection.source === 'custom'"
                        class="version-filter-detail custom-version-detail"
                        data-ui="community:custom-version"
                    >
                        <CommunityVersionFilter
                            v-model="query.mcVersion"
                            :versions="manifestVersions"
                            :loading="manifestLoading"
                            @change="customVersionChanged"
                        />
                        <p class="muted">
                            从列表选择，或输入完整版本后按 Enter；快照和自定义版本按原值查询。
                        </p>
                    </div>
                    <div
                        v-if="manualLoaderMismatch && supportsLoader"
                        class="version-loader-note"
                        role="status"
                    >
                        已保留手选的 {{ loaderSummary }}；该实例使用
                        {{ filterInstance?.loader || '原版' }}。<button
                            type="button"
                            class="btn btn-ghost btn-sm"
                            @click="useInstanceLoader"
                        >
                            使用实例加载器
                        </button>
                    </div>
                </section>

                <div data-ui="CommunityView:d516b82f0eb8" class="search-row">
                    <div class="search-input-wrap">
                        <input
                            data-ui="CommunityView:bc0450fd9c8f"
                            v-model="query.keyword"
                            class="input"
                            :placeholder="
                                query.kind === 'mod'
                                    ? '输入 MOD 中文名、常用别名或英文名（支持模糊）…'
                                    : '输入资源名称，回车搜索…'
                            "
                            @input="
                                onKeywordInput();
                                onKeywordSettled();
                            "
                            @keyup.enter="
                                suggestActive >= 0
                                    ? pickSuggestion(suggestions[suggestActive])
                                    : onSearch()
                            "
                            @keydown="onKeywordKeydown"
                            @blur="closeSuggest"
                        />
                        <!-- 输入联想下拉：词典命中 + 内置别名，键盘上下选择、回车采用 -->
                        <div
                            v-if="suggestOpen && suggestions.length"
                            class="suggest-pop"
                            role="listbox"
                        >
                            <button
                                v-for="(s, i) in suggestions"
                                :key="`${s.label}|${s.sublabel}`"
                                type="button"
                                class="suggest-item"
                                :class="{ active: i === suggestActive }"
                                role="option"
                                :aria-selected="i === suggestActive"
                                @mousedown.prevent="pickSuggestion(s)"
                            >
                                <span class="suggest-label">{{ s.label }}</span>
                                <span v-if="s.sublabel" class="suggest-sub">
                                    {{ s.sublabel }}
                                </span>
                                <span v-if="s.mcmodId" class="suggest-tag">百科</span>
                            </button>
                        </div>
                    </div>
                    <button
                        data-ui="CommunityView:ce39174e4563"
                        class="btn btn-gold search-btn"
                        :disabled="loading"
                        @click="onSearch"
                    >
                        <span
                            data-ui="CommunityView:bb1887897e8c"
                            v-if="loading"
                            class="spin"
                        ></span>
                        <svg
                            v-else
                            viewBox="0 0 24 24"
                            width="14"
                            height="14"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <circle cx="11" cy="11" r="7" />
                            <path d="m21 21-4.3-4.3" />
                        </svg>
                        搜索
                    </button>
                    <button
                        data-ui="CommunityView:15aecd36844d"
                        class="btn btn-ghost"
                        :disabled="loading"
                        @click="onReset"
                    >
                        重置条件
                    </button>
                </div>

                <div class="filter-row">
                    <SelectMenu
                        aria-label="资源来源"
                        v-model="query.source"
                        class="filter-select"
                        :options="sourceOptions"
                        @change="onFilterChange"
                    />
                    <SelectMenu
                        aria-label="加载器"
                        v-if="supportsLoader"
                        v-model="query.loader"
                        class="filter-select"
                        :options="loaderOptions"
                        @change="loaderChanged"
                    />
                    <SelectMenu
                        aria-label="分类"
                        v-model="query.category"
                        class="filter-select"
                        :options="categoryOptions"
                        @change="onFilterChange"
                    />
                    <SelectMenu
                        aria-label="排序"
                        v-model="query.sort"
                        class="filter-select"
                        :options="sortOptions"
                        @change="onFilterChange"
                    />
                </div>
            </div>

            <!-- 结果列表 -->
            <div data-ui="CommunityView:5e55abba5e8a" ref="listCard" class="card list-card">
                <div
                    data-ui="CommunityView:72423555b623"
                    v-if="results.length && (loading || loadError)"
                    class="status-strip"
                    role="status"
                >
                    {{
                        loading
                            ? '正在更新条件，暂时显示上次结果…'
                            : '更新失败，以下为上次结果：' + loadError
                    }}<button
                        data-ui="CommunityView:4ab4e45e43a0"
                        v-if="loadError"
                        class="btn btn-ghost btn-sm"
                        @click="doSearch(true)"
                    >
                        重试
                    </button>
                </div>
                <p
                    data-ui="CommunityView:fa2eef57b2fc"
                    v-for="warning in searchWarnings"
                    :key="warning"
                    class="search-warning"
                >
                    {{ warning }}
                </p>
                <!-- 加载中：与结果卡片网格同构，搜索完成后无缝过渡 -->
                <ContentSkeleton
                    v-if="loading && !results.length"
                    label="正在搜索社区资源…"
                    variant="grid"
                    :rows="6"
                    retry
                    @retry="doSearch(true)"
                />
                <!-- 错误态 -->
                <div
                    data-ui="CommunityView:680f71022a3b"
                    v-else-if="loadError && !results.length"
                    class="empty"
                >
                    <span>搜索失败：{{ loadError }}</span>
                    <button
                        data-ui="CommunityView:dadfdac6acc0"
                        class="btn btn-ghost btn-sm"
                        @click="usesPagination ? doSearch(false) : onSearch()"
                    >
                        重试
                    </button>
                </div>
                <!-- 空态 -->
                <div data-ui="CommunityView:170233f26efe" v-else-if="!results.length" class="empty">
                    <svg
                        viewBox="0 0 24 24"
                        width="40"
                        height="40"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <circle cx="12" cy="12" r="9" />
                        <path d="M3 12h18" />
                        <path d="M12 3a13.5 13.5 0 0 1 0 18" />
                        <path d="M12 3a13.5 13.5 0 0 0 0 18" />
                    </svg>
                    <span>{{
                        searched
                            ? '没有找到匹配的资源，换个关键词或条件试试'
                            : '输入关键词或选择条件开始搜索'
                    }}</span>
                </div>
                <!-- 列表 -->
                <template v-else>
                    <div
                        data-ui="CommunityView:ea58bb908a86"
                        class="result-list"
                        :class="{ 'swap-in': !!swapAnim }"
                        :inert="loading || !!loadError"
                        :aria-busy="loading"
                    >
                        <div
                            data-ui="CommunityView:0d40e298da83"
                            v-for="r in results"
                            :key="itemKey(r)"
                            class="result-card"
                        >
                            <div data-ui="CommunityView:491b5f81c088" class="result-top">
                                <div data-ui="CommunityView:00f58a1819fa" class="result-icon">
                                    <img
                                        data-ui="CommunityView:830997d3ecb7"
                                        v-if="r.iconUrl && !brokenIcons.has(itemKey(r))"
                                        :src="r.iconUrl"
                                        alt=""
                                        loading="lazy"
                                        @error="onIconError(r)"
                                    />
                                    <span
                                        data-ui="CommunityView:a6c25e1efd38"
                                        v-else
                                        class="icon-placeholder"
                                        >{{ (r.title || '?').charAt(0).toUpperCase() }}</span
                                    >
                                </div>
                                <div data-ui="CommunityView:d33af84ab969" class="result-head">
                                    <strong class="result-title" tabindex="0" :title="r.title">{{
                                        r.title
                                    }}</strong>
                                    <span
                                        data-ui="CommunityView:6f93851d6071"
                                        class="tag"
                                        :class="r.source === 'modrinth' ? 'tag-success' : 'tag-cf'"
                                    >
                                        来源：{{
                                            r.source === 'modrinth' ? 'Modrinth' : 'CurseForge'
                                        }}
                                    </span>
                                    <span
                                        data-ui="CommunityView:ad4b7d43f757"
                                        v-if="r.author"
                                        class="muted result-author"
                                        >{{ r.author }}</span
                                    >
                                </div>
                            </div>
                            <p
                                data-ui="CommunityView:2c1a48d38ee1"
                                class="result-desc"
                                :title="r.description"
                            >
                                {{ r.description || '暂无简介' }}
                            </p>
                            <div data-ui="CommunityView:b2d346eb1503" class="result-meta muted">
                                <span>下载量 {{ fmtDownloads(r.downloads) }}</span>
                                <span data-ui="CommunityView:e69c45508563" class="meta-dot">·</span>
                                <span>更新于 {{ fmtDate(r.updatedAt) }}</span>
                            </div>
                            <div data-ui="CommunityView:9195d6b103b6" class="result-foot">
                                <div data-ui="CommunityView:22746f0aa9cd" class="result-links">
                                    <button
                                        data-ui="CommunityView:7c3533e67ba3"
                                        class="icon-btn"
                                        :title="`打开 ${r.source === 'modrinth' ? 'Modrinth' : 'CurseForge'} 源页面（查看完整介绍）`"
                                        @click="openExternal(sourceUrl(r))"
                                    >
                                        <svg
                                            viewBox="0 0 24 24"
                                            width="14"
                                            height="14"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                        >
                                            <path
                                                d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"
                                            />
                                            <path d="M15 3h6v6" />
                                            <path d="M10 14 21 3" />
                                        </svg>
                                    </button>
                                    <button
                                        data-ui="CommunityView:0c667bb024b0"
                                        class="icon-btn"
                                        title="在 MC 百科查看介绍与教程"
                                        @click="openMcmod(r)"
                                    >
                                        <svg
                                            viewBox="0 0 24 24"
                                            width="14"
                                            height="14"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                        >
                                            <path
                                                d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"
                                            />
                                        </svg>
                                    </button>
                                    <button
                                        v-if="query.kind === 'mod'"
                                        class="icon-btn result-favorite"
                                        :class="{
                                            active: favorites.some((f) => f.key === itemKey(r))
                                        }"
                                        :aria-label="`${favorites.some((f) => f.key === itemKey(r)) ? '取消收藏' : '收藏'} ${r.title}`"
                                        :aria-pressed="favorites.some((f) => f.key === itemKey(r))"
                                        :title="
                                            favorites.some((f) => f.key === itemKey(r))
                                                ? '取消收藏模组'
                                                : '收藏模组'
                                        "
                                        :disabled="favoriteBusy.has(itemKey(r))"
                                        @click.stop="
                                            toggleProject(r.source, r.projectId, r.title, r.iconUrl)
                                        "
                                    >
                                        <svg
                                            viewBox="0 0 24 24"
                                            width="16"
                                            height="16"
                                            :fill="
                                                favorites.some((f) => f.key === itemKey(r))
                                                    ? 'currentColor'
                                                    : 'none'
                                            "
                                            stroke="currentColor"
                                            stroke-width="1.7"
                                            stroke-linejoin="round"
                                        >
                                            <path
                                                d="m12 3 2.8 5.7 6.3.9-4.6 4.4 1.1 6.3-5.6-3-5.6 3 1.1-6.3L3 9.6l6.2-.9Z"
                                            />
                                        </svg>
                                    </button>
                                </div>
                                <button
                                    data-ui="CommunityView:6d17d47c8729"
                                    class="btn btn-gold btn-sm result-dl"
                                    @click="openDownload(r)"
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        width="13"
                                        height="13"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                    >
                                        <path d="M12 3v11" />
                                        <path d="m7 10 5 5 5-5" />
                                        <path d="M4 21h16" />
                                    </svg>
                                    下载
                                </button>
                            </div>
                        </div>
                    </div>
                    <!-- 无限滚动哨兵：进入视口自动加载更多（按钮保留作兜底） -->
                    <div
                        data-ui="CommunityView:1833f0867a9c"
                        v-if="!usesPagination && hasMore && !loading && !loadError"
                        ref="moreSentinel"
                        class="more-sentinel"
                    ></div>
                    <!-- 加载更多 -->
                    <div
                        data-ui="CommunityView:c18472307641"
                        v-if="!usesPagination && hasMore"
                        class="more-row"
                    >
                        <button
                            data-ui="CommunityView:165371e35396"
                            class="btn btn-ghost"
                            :disabled="loadingMore"
                            @click="onLoadMore"
                        >
                            <span
                                data-ui="CommunityView:6fa61ac3821a"
                                v-if="loadingMore"
                                class="spin"
                            ></span>
                            {{ loadingMore ? '加载中…' : '加载更多' }}
                        </button>
                    </div>
                </template>
                <nav
                    data-ui="CommunityView:d47a3ad6ba73"
                    v-if="usesPagination && searched && totalPages > 1"
                    class="pagination"
                    aria-label="资源分页"
                >
                    <span class="muted"
                        >共 {{ totalResults }} 项 · 第 {{ currentPage }} / {{ totalPages }} 页</span
                    >
                    <button
                        data-ui="CommunityView:391d4872befc"
                        class="btn btn-ghost btn-sm"
                        :disabled="loading || currentPage <= 1"
                        @click="goToPage(currentPage - 1)"
                    >
                        上一页
                    </button>
                    <button
                        data-ui="CommunityView:79ac33e57fdc"
                        v-for="page in visiblePages"
                        :key="page"
                        class="btn btn-sm"
                        :class="page === currentPage ? 'btn-gold' : 'btn-ghost'"
                        :aria-current="page === currentPage ? 'page' : undefined"
                        :disabled="loading"
                        @click="goToPage(page)"
                    >
                        {{ page }}
                    </button>
                    <button
                        data-ui="CommunityView:2f9290893a14"
                        class="btn btn-ghost btn-sm"
                        :disabled="loading || currentPage >= totalPages"
                        @click="goToPage(currentPage + 1)"
                    >
                        下一页
                    </button>
                </nav>
            </div>
        </template>
        <!-- 下载模态框 -->
        <Teleport to="body">
            <div
                data-ui="CommunityView:ef88acc39749"
                v-if="modal.open"
                class="modal-mask"
                @pointerdown.self="modal.open = false"
            >
                <div data-ui="CommunityView:6904c547ed30" class="modal download-modal">
                    <h3 data-ui="CommunityView:7b81ed690844" class="modal-title">
                        <MarqueeText :text="'下载 ' + modal.item?.title" />
                    </h3>
                    <div data-ui="CommunityView:a84b1e456827" v-if="modal.item" class="modal-links">
                        <button
                            v-if="modal.kind === 'mod'"
                            class="btn btn-ghost btn-sm"
                            :disabled="favoriteBusy.has(itemKey(modal.item))"
                            :aria-pressed="favorites.some((f) => f.key === itemKey(modal.item!))"
                            @click="
                                toggleProject(
                                    modal.item.source,
                                    modal.item.projectId,
                                    modal.item.title,
                                    modal.item.iconUrl
                                )
                            "
                        >
                            <UiGlyph
                                name="star"
                                :size="14"
                                :filled="favorites.some((f) => f.key === itemKey(modal.item!))"
                            />{{
                                favorites.some((f) => f.key === itemKey(modal.item!))
                                    ? ' 已收藏'
                                    : ' 收藏模组'
                            }}
                        </button>
                        <button
                            data-ui="CommunityView:6fabba70cd3a"
                            class="btn btn-ghost btn-sm"
                            :title="'在启动器内打开源页面'"
                            @click="
                                openInline(
                                    sourceUrl(modal.item, modal.kind),
                                    modal.item.title || '源页面'
                                )
                            "
                        >
                            {{
                                modal.item.source === 'modrinth'
                                    ? 'Modrinth 源页面'
                                    : 'CurseForge 源页面'
                            }}
                        </button>
                        <button
                            data-ui="CommunityView:source-external"
                            class="icon-btn"
                            title="用系统浏览器打开"
                            aria-label="用系统浏览器打开源页面"
                            @click="openExternal(sourceUrl(modal.item, modal.kind))"
                        >
                            <UiGlyph name="expand" :size="14" />
                        </button>
                        <button
                            data-ui="CommunityView:03eb0c52ad3e"
                            class="btn btn-ghost btn-sm"
                            @click="openMcmod(modal.item)"
                        >
                            MC 百科介绍
                        </button>
                    </div>
                    <div class="filter-row">
                        <label data-ui="CommunityView:9b7baa1d1a72" class="modal-field"
                            >Minecraft 版本<CommunityVersionFilter
                                v-model="modal.mcVersion"
                                :versions="manifestVersions"
                                :loading="manifestLoading"
                                @change="loadFiles"
                        /></label>
                        <label
                            data-ui="CommunityView:73416dae43e4"
                            v-if="usesCommunityLoader(modal.kind)"
                            class="modal-field"
                            >Loader<SelectMenu
                                v-model="modal.loader"
                                :options="loaderOptions"
                                @change="loadFiles"
                        /></label>
                    </div>

                    <p class="modal-label">选择文件版本</p>
                    <div
                        data-ui="CommunityView:8f95d8f66a15"
                        v-if="modal.loadingFiles"
                        class="files-loading"
                    >
                        <span data-ui="CommunityView:8941adbc1d4f" class="spin"></span>
                        <span class="muted">正在获取文件列表…</span>
                    </div>
                    <template v-else>
                        <div
                            data-ui="CommunityView:db1154820afe"
                            v-if="modal.files.length"
                            class="file-list"
                        >
                            <button
                                data-ui="CommunityView:ceffac990295"
                                v-for="f in modal.files"
                                :key="f.fileId"
                                class="file-row"
                                :class="{ active: modal.fileId === f.fileId }"
                                @click="modal.fileId = f.fileId"
                            >
                                <span data-ui="CommunityView:05bff2e14ac9" class="file-main">
                                    <span
                                        data-ui="CommunityView:3906a840cd50"
                                        class="file-name"
                                        :title="f.fileName"
                                        >{{ f.fileName }}</span
                                    >
                                    <span data-ui="CommunityView:a7eecc7757d6" class="file-sub"
                                        >版本 {{ f.version }} · MC {{ f.gameVersions.join(' / ')
                                        }}<template
                                            v-if="
                                                usesCommunityLoader(modal.kind) && f.loaders.length
                                            "
                                        >
                                            · {{ f.loaders.join(' / ') }}</template
                                        ></span
                                    >
                                </span>
                                <span data-ui="CommunityView:9584cb689677" class="file-side">
                                    <span
                                        data-ui="CommunityView:44e7a728a5f3"
                                        class="tag"
                                        :class="releaseTagClass(f.releaseType)"
                                        >{{ releaseText[f.releaseType] }}</span
                                    >
                                    <span
                                        data-ui="CommunityView:32e67d25368a"
                                        class="muted file-meta"
                                        >{{ fmtDate(f.date) }} · {{ fmtSize(f.size) }}</span
                                    >
                                </span>
                            </button>
                        </div>
                        <p
                            data-ui="CommunityView:579adeba801a"
                            v-if="modal.filesError"
                            class="files-error"
                        >
                            {{ modal.filesError }}
                        </p>
                    </template>

                    <!-- 目标版本（整合包安装即新实例，无需选择） -->
                    <template v-if="!isModpack">
                        <p class="modal-label">下载到版本</p>
                        <div class="target-row">
                            <SelectMenu
                                aria-label="下载目标实例"
                                v-if="targetOptions.length && !modal.externalDir"
                                v-model="modal.versionId"
                                :options="
                                    targetOptions.map((v) => ({
                                        value: instanceKey(v),
                                        label:
                                            v.id +
                                            ' · ' +
                                            v.mcVersion +
                                            ' / ' +
                                            (v.loader || '纯净版') +
                                            ' · ' +
                                            v.folder
                                    }))
                                "
                            />
                            <span v-if="modal.externalDir" class="muted external-dir" :title="modal.externalDir">{{
                                modal.externalDir
                            }}</span>
                            <button
                                class="btn btn-ghost btn-sm"
                                :aria-label="modal.externalDir ? '改回下载到实例' : '下载到任意文件夹'"
                                :title="
                                    modal.externalDir
                                        ? '改回下载到实例'
                                        : '不进实例，直接保存到所选文件夹'
                                "
                                @click="
                                    modal.externalDir
                                        ? ((modal.externalDir = ''), (modal.versionId = ''))
                                        : pickExternalDir()
                                "
                            >
                                {{ modal.externalDir ? '改回实例' : '文件夹…' }}
                            </button>
                        </div>
                        <p
                            data-ui="CommunityView:ab12acbb18fe"
                            v-if="!targetOptions.length && !modal.externalDir"
                            class="files-error"
                        >
                            没有与所选文件兼容的已安装实例；可调整文件筛选、下载到任意文件夹，或在游戏版本页安装。
                        </p>
                    </template>
                    <p data-ui="CommunityView:a8e08b82f315" v-else class="muted pack-tip">
                        整合包将下载后自动创建独立实例并安装
                    </p>

                    <div data-ui="CommunityView:2356b94bbc0d" class="modal-actions">
                        <button
                            data-ui="CommunityView:989d28842ec5"
                            class="btn btn-ghost"
                            @click="modal.open = false"
                        >
                            取消
                        </button>
                        <button
                            data-ui="CommunityView:cade5c4fc83a"
                            class="btn btn-gold"
                            :disabled="!canConfirm"
                            @click="confirmDownload"
                        >
                            {{ modal.kind === 'mod' ? '加入队列并检测' : '确认下载' }}
                        </button>
                    </div>
                </div>
            </div>
        </Teleport>
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

<style scoped>
.community-sections {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px;
    /* 分段控件：外圈靠描边成组，选中项自己带底，不再整体垫一层 */
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    align-self: flex-start;
}
.community-section {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 36px;
    border: 0;
    border-radius: var(--radius-sm);
    padding: 0 16px;
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    transition:
        background 160ms,
        color 160ms;
}
.community-section.active {
    background: var(--accent-soft);
    color: var(--accent-2);
    font-weight: 600;
}
.community-section span {
    display: grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    /* 计数徽标：半透明淡底即可，避免又一个小实心块 */
    background: color-mix(in srgb, var(--card-solid, var(--card)) 45%, transparent);
    border-radius: 6px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
}
.community-section:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
}
.version-filter-panel {
    display: grid;
    gap: var(--space-3);
    padding: 0 0 var(--space-4);
    /* 卡内区块：靠「底部一条分隔线 + 间距」分组，既不再垫第二层底，
       也不再用整圈描边把自己变成「卡中卡」。 */
    border: 0;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
    border-radius: 0;
    background: transparent;
}
.version-filter-heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: var(--text-sm);
}
.version-filter-heading .muted {
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
}
.version-source-options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
}
.version-source-option {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    min-height: var(--ctl-h);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--card);
    color: var(--text-dim);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
}
.version-source-option:hover {
    background: var(--hover);
    color: var(--text);
}
.version-source-option.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
    font-weight: 600;
}
.version-source-option:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
}
.version-source-option span {
    min-width: 1.4em;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    background: var(--hover);
    font-size: var(--text-xs);
}
.version-filter-detail {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    min-width: 0;
}
.version-filter-detail :deep(.instance-filter) {
    flex: 1;
    min-width: min(230px, 100%);
    max-width: 100%;
}
.version-filter-detail .muted {
    flex: 1;
    margin: 0;
    font-size: var(--text-xs);
    line-height: 1.5;
}
.custom-version-detail :deep(.community-version-filter) {
    flex: 1;
    min-width: min(170px, 100%);
}
.version-loader-note {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    color: var(--text-dim);
    font-size: var(--text-xs);
}
/* 页面骨架与宽度由全局 .page.page--content 提供（UI_SYSTEM.md §4） */

/* ---------------- 搜索卡片 ---------------- */
.search-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
}
.search-row {
    display: flex;
    gap: var(--space-3);
}
.search-row .input {
    flex: 1;
    min-width: 0;
}
.search-btn {
    flex-shrink: 0;
}

.kind-capsules {
    position: relative;
    display: flex;
    gap: 2px;
    flex-wrap: wrap;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 999px;
    /* 同上：实心胶囊已经靠 blob 高亮，外圈不再垫第二层底 */
    background: transparent;
    width: fit-content;
}
/* Selection follows the active category with the shared deceleration curve. */
.capsule-blob {
    position: absolute;
    border-radius: 999px;
    background: var(--accent-grad);
    box-shadow: 0 2px 8px var(--accent-soft);
    transition:
        left var(--motion-normal) var(--ease-out),
        top var(--motion-normal) var(--ease-out),
        width var(--motion-normal) var(--ease-out),
        height 0.32s ease,
        opacity 0.15s ease;
    pointer-events: none;
    z-index: 0;
}
.capsule {
    position: relative;
    z-index: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: calc(var(--ctl-h) - 6px);
    padding: 0 var(--space-4);
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-dim);
    font-size: var(--text-sm);
    font-family: inherit;
    cursor: pointer;
    white-space: nowrap;
    transition: color 0.2s ease;
}
.capsule:hover {
    color: var(--text);
}
.capsule.active {
    color: var(--on-accent);
    font-weight: 600;
}

.filter-row {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
}
:deep(.filter-select) {
    width: auto;
    flex: 1;
    min-width: 140px;
}

/* ---------------- 结果列表（卡片横向网格，窄窗口自动换行） ---------------- */
.list-card {
    padding: 0;
    background: transparent;
    border: 0;
    box-shadow: none;
}
.pagination {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-4) 0 var(--space-2);
}
.search-warning {
    color: var(--text-dim);
    font-size: var(--text-xs);
    padding: var(--space-2);
}
.result-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 260px), 1fr));
    gap: var(--space-3);
}
.result-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--card);
    transition:
        border-color var(--motion-fast) ease,
        box-shadow var(--motion-normal) ease;
    /* A single fade keeps filtering and paging visually immediate. */
    animation: community-card-in var(--motion-enter) var(--ease-out) backwards;
}
@keyframes community-card-in {
    from {
        opacity: 0;
    }
    to {
        opacity: 1;
    }
}
.result-card:hover {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    box-shadow: var(--shadow);
}

.result-top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
}

.result-icon {
    width: 46px;
    height: 46px;
    flex-shrink: 0;
    border-radius: var(--radius-md);
    overflow: hidden;
    /* 不再给图标底再叠一层 --card：与卡片同色 → 读成卡里嵌了块脏色块。
       图标本身就带自己的底色/透明底，留空即可（与模组页图标块同口径）。 */
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
}
.result-icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
}
.icon-placeholder {
    font-size: var(--text-lg);
    font-weight: 700;
    color: var(--text-dim);
}

.result-head {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
}
.result-title {
    flex: 1 1 100%;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    line-height: 1.45;
    font-weight: 650;
    font-size: var(--text-md);
}
/* CurseForge 橙（Modrinth 绿复用 tag-success） */
.tag-cf {
    background: color-mix(in srgb, #f97316 12%, transparent);
    color: #f97316;
}
.result-author {
    font-size: var(--text-xs);
    min-width: 0;
}
.result-desc {
    font-size: var(--text-xs);
    line-height: 1.6;
    color: var(--text-dim);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    /* 固定两行高度，保证网格内卡片对齐不跳动 */
    min-height: calc(var(--text-xs) * 1.6 * 2);
}
.result-meta {
    font-size: var(--text-xs);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    margin-top: auto;
}
.meta-dot {
    opacity: 0.6;
}
.result-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
}
.result-links {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
}
.result-favorite.active {
    color: var(--accent-2);
    background: var(--accent-soft);
}
.result-favorite:disabled {
    opacity: 0.55;
    cursor: wait;
}
.modal-links {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
}
.result-dl {
    flex-shrink: 0;
}

.more-row {
    display: flex;
    justify-content: center;
    padding: var(--space-4) 0 var(--space-2);
}

/* ---------------- 下载模态框 ---------------- */
.download-modal {
    width: min(740px, calc(100vw - 40px));
    max-height: 88vh;
    overflow-y: auto;
}
.modal-field {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-dim);
}
/* 弹窗里的版本选择器复用页面级组件：它在 grid 里没有 flex 语境，按列铺满 */
.modal-field :deep(.community-version-filter) {
    flex: none;
    width: 100%;
    min-width: 0;
}
.modal-title {
    font-size: var(--text-lg);
    font-weight: 700;
    margin: 0 0 var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.modal-label {
    font-size: var(--text-sm);
    color: var(--text-dim);
    margin: var(--space-4) 0 var(--space-2);
}
/* 目标行：实例选择器 + 外部目录开关/回显 */
.target-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
}
.target-row > :first-child {
    flex: 1;
    min-width: 0;
}
.external-dir {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
}
.files-loading {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4) 0;
}
/* 文件版本列表：卡片化行（主行文件名 + 副行版本兼容信息，右侧标签+日期体积），宽松呼吸 */
.file-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 280px;
    overflow-y: auto;
    padding: var(--space-1) var(--space-1) var(--space-1) 0;
    /* 内嵌滚动区不再用外框包住（卡片自带边界） */
}
.file-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
    color: var(--text);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition:
        border-color 0.16s ease,
        background 0.16s ease,
        transform 0.16s ease,
        box-shadow 0.2s ease;
}
.file-row:hover {
    background: var(--hover);
    border-color: var(--border-strong);
    transform: translateY(-1px);
}
.file-row.active {
    background: var(--accent-soft);
    border-color: var(--accent);
    box-shadow:
        inset 0 0 0 1px var(--accent),
        0 4px 14px var(--accent-soft);
}
/* 左：文件名（主）+ 版本兼容信息（副） */
.file-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
}
.file-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 650;
    font-size: var(--text-sm);
    font-family: ui-monospace, Consolas, monospace;
}
.file-sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
    color: var(--text-dim);
}
/* 右：发行标签 + 日期·体积 */
.file-side {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
}
.file-meta {
    font-size: var(--text-xs);
    white-space: nowrap;
}
.files-error {
    font-size: var(--text-sm);
    color: var(--danger);
    padding: var(--space-1) 0;
}
.pack-tip {
    font-size: var(--text-sm);
    margin: var(--space-4) 0 0;
}
.modal-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    margin-top: var(--space-5);
}
.search-card {
    padding: var(--card-pad);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
}
.search-row {
    margin: 0;
}
.instance-row {
    margin: 0;
    padding: 0;
    justify-content: flex-end;
}
.instance-row .instance-filter {
    max-width: 420px;
}
.filter-row {
    gap: 12px;
}
.result-list {
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr));
    gap: 16px;
}
/* 无感刷新：条件变化重搜时旧结果保留在位，新结果就位后整列轻淡入（不闪白不跳动） */
.result-list.swap-in {
    animation: results-swap-in 0.18s ease;
}
@keyframes results-swap-in {
    from {
        opacity: 0.35;
    }
    to {
        opacity: 1;
    }
}
.result-card {
    box-shadow: none;
    padding: 16px;
    min-width: 0;
}
.result-title {
    font-size: 16px;
    line-height: 1.45;
    white-space: normal;
    word-break: normal;
    overflow-wrap: break-word;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    max-height: none;
    min-height: 0;
}
.result-head {
    min-width: 0;
    gap: 6px;
}
.result-desc {
    line-height: 1.6;
    min-height: 3.2em;
    -webkit-line-clamp: 2;
}
.result-head .tag {
    font-size: 11px;
    /* 标签读作「说明文字」，不读作「卡里的小卡」：给极淡底 + 描边即可 */
    background: color-mix(in srgb, var(--card-solid, var(--card)) 55%, transparent);
    color: var(--text-dim);
    border: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
}
.result-links .icon-btn {
    width: 34px;
    height: 34px;
}
.result-foot {
    gap: 12px;
}
.result-author {
    font-size: 12px;
}
.result-meta {
    font-size: 12px;
}
.capsule {
    border: 0 !important;
}
.capsule-blob {
    background: var(--accent-soft) !important;
    box-shadow: none !important;
}
@media (max-width: 800px) {
    .instance-row {
        flex-wrap: wrap;
    }
    .instance-row .instance-filter {
        max-width: none;
        width: 100%;
    }
    .search-card {
        padding: 12px;
    }
}
.community-page {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px !important;
}
.community-page > .page-head {
    margin: 0;
}
.community-page > .instance-row {
    justify-content: flex-start;
}
.community-page .search-card {
    margin: 0;
}
.community-page .kind-capsules {
    margin: 0;
    /* 恢复 base 的对称内衬：0 0 8px 会让首/尾胶囊顶着容器圆角，滑动指示块出界 */
    padding: 3px;
}
.community-page .search-card .filter-row {
    margin: 0;
}
.community-page .result-title {
    font-size: 16px;
}
@media (min-width: 1450px) {
    .community-page {
        grid-template-columns: minmax(0, 1fr) minmax(360px, 1fr);
    }
    .community-page > .instance-row {
        justify-content: flex-end;
    }
    /* 两栏只服务于「找资源」分支；分区条、下载队列、收藏管理都跨满整行 */
    .community-page > .search-card,
    .community-page > .card,
    .community-page > .status-strip,
    .community-page > .community-sections,
    .community-page > .favorites-manager {
        grid-column: 1/-1;
    }
}

.community-page .capsule.active {
    color: var(--text);
    background: var(--accent-soft);
    text-shadow: none;
}
/* ---------- 搜索联想下拉 ---------- */
.search-input-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
}
.search-input-wrap > .input {
    width: 100%;
}
.suggest-pop {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    padding: 6px;
    max-height: 300px;
    overflow-y: auto;
    background: color-mix(in srgb, var(--card-solid, var(--card)) 97%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    backdrop-filter: blur(18px);
    -webkit-backdrop-filter: blur(18px);
}
.suggest-item {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-radius: var(--radius-sm, 8px);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
}
.suggest-item:hover,
.suggest-item.active {
    background: var(--accent-soft);
}
.suggest-label {
    flex-shrink: 0;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
}
.suggest-sub {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-dim);
    font-size: var(--text-xs);
}
.suggest-tag {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 11px;
}
</style>
