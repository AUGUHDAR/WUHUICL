<script setup lang="ts">
import { openInstanceCenter } from '../instanceCenter';
import { appearancePreview } from '../visualDesign';
import BrandMicrosoft from '../components/BrandMicrosoft.vue';
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { activeCarouselKeys, carouselDuration } from '@shared/appearancePolicy';
import { useMotion } from '../motion';
import { CarouselPlayback } from '@shared/carouselPlayback';
import {
    errText,
    exportLaunchLogs,
    exportLaunchScript,
    shareLogsToMclogs,
    getSkinProfile,
    getSettings,
    launchGame,
    restartGame,
    cancelGameRestart,
    listJava,
    setVersionJava,
    openDir,
    removeVersion,
    setActiveFolder,
    showFolderContextMenu
} from '../api';
import {
    selectedInstance,
    activeInstalled,
    selectInstance,
    displayVersionName,
    displayVersionSub,
    fmtLastPlayed,
    isFavorite,
    openSettings,
    progressMono,
    refreshInstalled,
    sortWithFavorite,
    store,
    toast,
    toggleFavorite,
    versionIconUrl
} from '../store';
import Avatar from '../components/Avatar.vue';
import { skinSiteIconUrl } from '../skinSiteIcons';
import AppMenu, { type AppMenuItem } from '../components/AppMenu.vue';
import AppModal from '../components/AppModal.vue';
import ConfirmModal from '../components/ConfirmModal.vue';
import EmptyState from '../components/EmptyState.vue';
import UiGlyph from '../components/UiGlyph.vue';
import HeroSceneCanvas from '../components/HeroSceneCanvas.vue';
import type { HeroSceneEntry } from '../heroScenes';
import SkinViewer3D from '../components/SkinViewer3D.vue';
import CreatorCard from '../components/CreatorCard.vue';
import LaunchHeatmap from '../components/LaunchHeatmap.vue';
// 手机触屏没有 hover 引导，死按钮等于断路：无实例时按钮保持可点并给出引导
const isAndroid = window.kamucl.platform === 'android';
import type {
    ImageFit,
    InstalledVersion,
    JavaInfo,
    ProfileSkins,
    SkinVariant
} from '@shared/types';
import { trackBootTask } from '../bootTasks';
import { managedImageUrl } from '../managedAssets';
import { builtInLaunchImages } from '../launchImages';

const LAST_VERSION_KEY = 'kamucl.lastVersion';

// ---------------- 当前实例与展示图 ----------------
const selectedId = computed({
    get: () => store.resourceVersionId,
    set: (id) => {
        store.resourceVersionId = id;
    }
});
const currentVersion = selectedInstance;
const versionLabel = (version: InstalledVersion) => displayVersionName(version);
const cap = (value: string) => value.charAt(0).toUpperCase() + value.slice(1);
const loaderText = (version: InstalledVersion) =>
    version.loader
        ? `${version.loader === 'neoforge' ? 'NeoForge' : cap(version.loader)} ${version.loaderVersion ?? ''}`.trim()
        : // 原版实例按版本形态判型：快照挂「正式版」徽章=自相矛盾（26.4-snapshot-5 / 23w14a / rc）
          vanillaTypeText(version.mcVersion);
const vanillaTypeText = (mcVersion?: string) => {
    const v = mcVersion ?? '';
    if (/snapshot/i.test(v) || /^\d{2}w\d{1,2}[a-z]/i.test(v)) return '快照版';
    if (/-pre\d/i.test(v) || /-rc\d/i.test(v)) return '预发布版';
    return '正式版';
};
// 与下方实例卡片共用命名规则；技术版本只读真实元数据，不从名称推断。
const heroName = computed(() =>
    currentVersion.value ? versionLabel(currentVersion.value) : '选择游戏实例'
);
const heroVersion = computed(() => currentVersion.value?.mcVersion || '版本未知');

function fitCss(fit: ImageFit): 'fill' | 'contain' | 'cover' {
    return fit === 'fill' ? 'fill' : fit === 'fit' ? 'contain' : 'cover';
}

const failedBanners = ref(new Set<string>());
const globalBanners = computed(() => {
    const global = appearancePreview.value?.launchThumbnail;
    return activeCarouselKeys(global).map((path) => {
        const bundled = builtInLaunchImages.find((image) => image.key === path);
        return {
            path,
            src: bundled?.src ?? managedImageUrl(path),
            fit: fitCss(global?.fit ?? 'crop'),
            custom: !bundled
        };
    });
});
const instanceBanners = computed(() => {
    const version = currentVersion.value;
    if (version?.thumbnail) {
        return [
            {
                path: version.thumbnail,
                src: managedImageUrl(version.thumbnail),
                fit: fitCss(version.thumbnailFit ?? 'crop'),
                custom: true
            }
        ];
    }
    return [];
});

const banners = computed(() => {
    const instance = instanceBanners.value.filter((item) => !failedBanners.value.has(item.path));
    return instance.length
        ? instance
        : globalBanners.value.filter((item) => !failedBanners.value.has(item.path));
});
const { decorativeActive } = useMotion();
watch(decorativeActive, (active) => (active ? startBannerTimer() : stopBannerTimer()));
const bannerIndex = ref(0);
const outgoingBanner = ref<number | null>(null);
const upcomingBanner = ref(0);
const bannerLayers = computed(() =>
    [...new Set([bannerIndex.value, upcomingBanner.value, outgoingBanner.value])]
        .filter((index): index is number => index !== null && !!banners.value[index])
        .sort((a, b) => a - b)
        .map((index) => ({ ...banners.value[index], index }))
);
let bannerTimer: ReturnType<typeof setInterval> | null = null;
let playback: CarouselPlayback | null = null;
let playbackKey = '';
/** Only current/next/outgoing DOM images own decoded resources. */
const readyBanners = new Set<string>();
function syncBannerResources() {
    upcomingBanner.value = playback?.upcomingIndex() ?? bannerIndex.value;
    const retained = new Set(bannerLayers.value.map((item) => item.src));
    for (const src of readyBanners) if (!retained.has(src)) readyBanners.delete(src);
}
function finishBannerExit(index: number, event: TransitionEvent) {
    if (
        event.propertyName === 'opacity' &&
        outgoingBanner.value === index &&
        bannerIndex.value !== index
    ) {
        outgoingBanner.value = null;
        syncBannerResources();
    }
}
const bannerScope = computed(() =>
    instanceBanners.value.some((item) => !failedBanners.value.has(item.path))
        ? `instance:${currentVersion.value?.folder}:${currentVersion.value?.id}`
        : 'global'
);

function stopBannerTimer() {
    if (playback && playbackKey) {
        try {
            localStorage.setItem(playbackKey, JSON.stringify(playback.bookmark(Date.now())));
        } catch {
            /* read-only storage */
        }
    }
    if (bannerTimer) clearInterval(bannerTimer);
    bannerTimer = null;
}

function startBannerTimer() {
    stopBannerTimer();
    outgoingBanner.value = null;
    if (showHeroAnimation.value) {
        // 动画模式不渲染图片层；书签在切回图片模式时由下方 watch 重启轮播续播。
        return;
    }
    if (!banners.value.length) {
        playback = null;
        playbackKey = '';
        bannerIndex.value = 0;
        upcomingBanner.value = 0;
        readyBanners.clear();
        return;
    }
    playbackKey =
        'kamucl.carousel.' +
        bannerScope.value +
        (appearancePreview.value?.launchThumbnail.randomPlayback ? '.random' : '');
    let saved;
    try {
        saved = JSON.parse(localStorage.getItem(playbackKey) ?? 'null');
    } catch {
        /* invalid bookmark */
    }
    const settings = appearancePreview.value?.launchThumbnail;
    playback = new CarouselPlayback(
        banners.value.map((item) => ({
            path: item.path,
            durationMs:
                1000 *
                carouselDuration(settings?.durations?.[item.path] ?? settings?.intervalSeconds)
        })),
        Date.now(),
        saved,
        settings?.randomPlayback === true
    );
    bannerIndex.value = playback.index;
    syncBannerResources();
    if (banners.value.length < 2 || !decorativeActive.value) return;
    bannerTimer = setInterval(() => {
        if (document.hidden || !playback) return;
        const nextIdx = playback.peekNext(Date.now());
        if (nextIdx !== playback.index && !readyBanners.has(banners.value[nextIdx]?.src ?? ''))
            return; // 下一张未就绪，下一拍再试
        const index = playback.tick(Date.now());
        if (index !== bannerIndex.value) {
            outgoingBanner.value = bannerIndex.value;
            bannerIndex.value = index;
            syncBannerResources();
        }
    }, 100);
}

watch(
    () =>
        JSON.stringify([
            instanceBanners.value,
            globalBanners.value,
            appearancePreview.value?.launchThumbnail.intervalSeconds,
            appearancePreview.value?.launchThumbnail.durations,
            appearancePreview.value?.launchThumbnail.randomPlayback
        ]),
    () => {
        failedBanners.value = new Set();
        startBannerTimer();
    }
);

function onBannerError(item: { custom: boolean; path: string }) {
    if (failedBanners.value.has(item.path)) return;
    failedBanners.value = new Set([...failedBanners.value, item.path]);
    bannerIndex.value = 0;
    startBannerTimer();
    toast('已跳过不可用的启动卡图片；仅显示已勾选且可用的图片', 'error');
}

// ---------------- 启动、设置与日志 ----------------
const launching = computed(() => store.launchState?.status === 'launching');
const running = computed(() => store.launchState?.status === 'running');
// 进程已拉起、游戏窗口尚未出现（PCL 同款阶段）
const waitingWindow = computed(() => store.launchState?.status === 'waiting-window');
const launchFailed = computed(
    () =>
        store.launchState?.status === 'error' ||
        (store.launchState?.status === 'exited' && store.launchState.code !== 0)
);
const percent = computed(() =>
    store.progress ? Math.round(progressMono(store.progress) * 100) : 0
);
const launchText = computed(() => {
    if (launching.value || waitingWindow.value) return store.progress?.text || '正在启动…';
    return running.value ? '再次启动' : launchFailed.value ? '重新启动' : '开始游戏';
});

// ---------------- 首页启动卡像素动画（背景类型 = 像素动画时替代图片轮播） ----------------
// 场景渲染在后端 hero_scenes（vendor 自 LumilioCL，起飞为 WUHUICL 原创）；
// HeroSceneCanvas 持有时钟并画帧，这里只决定模式、联动悬停/启动进度。
const heroAnimationMode = computed(() => appearancePreview.value?.launchThumbnail?.mode !== 'image');
const heroFallback = ref(false);
const showHeroAnimation = computed(() => heroAnimationMode.value && !heroFallback.value);
const heroHovered = ref(false);
const launchHovered = ref(false);
/** 当前场景信息行（HeroSceneCanvas 上报；背景类型为像素动画时显示在卡片左侧）。 */
const heroSceneInfo = ref<HeroSceneEntry | null>(null);
const heroLoading = computed(() => ({
    active: launching.value || waitingWindow.value,
    progress: Math.min(1, Math.max(0, percent.value / 100))
}));
const heroDimmed = computed(() => running.value);
const heroAnimationSpeed = computed(() => {
    const speed = appearancePreview.value?.launchThumbnail.animation?.speed;
    return typeof speed === 'number' && speed > 0 ? Math.min(3, speed) : 1;
});
const heroDwellSeconds = computed(() => {
    const dwell = appearancePreview.value?.launchThumbnail.animation?.dwellSeconds;
    return typeof dwell === 'number' && dwell > 0 ? Math.min(60, dwell) : 9.5;
});
const heroInstanceKey = computed(
    () => `${currentVersion.value?.folder ?? ''}/${currentVersion.value?.id ?? ''}`
);
function onHeroFallback(reason: string) {
    heroFallback.value = true;
    toast('像素动画暂不可用，已切回图片轮播：' + reason, 'error');
}
watch(showHeroAnimation, (active) => {
    // 动画↔图片模式切换：回到图片模式时重启轮播（含书签续播）。
    if (!active) startBannerTimer();
});
watch(heroAnimationMode, (mode) => {
    if (mode) heroFallback.value = false; // 切回动画模式时重置，给渲染器一次重试
});

// ---------------- 快捷行悬浮浮块（跟随指针在三格间平滑滑动） ----------------
const runtimeHover = ref(-1);
const runtimeStrip = ref<HTMLElement | null>(null);
const runtimeBlob = reactive({ left: 0, width: 0 });
function updateRuntimeBlob() {
    const strip = runtimeStrip.value;
    if (!strip || runtimeHover.value < 0) return;
    const items = strip.querySelectorAll<HTMLElement>('.runtime-item');
    const target = items[runtimeHover.value];
    if (!target) return;
    runtimeBlob.left = target.offsetLeft;
    runtimeBlob.width = target.offsetWidth;
}
watch(runtimeHover, () => nextTick(updateRuntimeBlob));
const runtimeBlobStyle = computed(() => ({
    left: runtimeBlob.left + 'px',
    width: runtimeBlob.width + 'px'
}));

const heroStatus = computed(() => {
    const version = currentVersion.value;
    if (!version) return { text: '等待选择', tone: 'idle' };
    if (running.value) return { text: '游戏运行中', tone: 'running' };
    if (waitingWindow.value) return { text: '正在等待窗口出现', tone: 'running' };
    if (launching.value) return { text: '正在准备', tone: 'running' };
    if (launchFailed.value || version.failed || version.incomplete) {
        return { text: '需要检查', tone: 'error' };
    }
    return { text: '就绪', tone: 'ready' };
});

const relaunchConfirm = ref<{ id: string; createCommandWorld: boolean } | null>(null);
async function startVersion(id: string, createCommandWorld = false, force = false) {
    if (!id) return;
    selectedId.value = id;
    // 多开支持：「正在启动/等待窗口」重复点击拦截；已有游戏运行中再启动需二次确认
    if (launching.value || waitingWindow.value) {
        toast('正在启动中，请稍候', 'info');
        return;
    }
    if (running.value && !force) {
        relaunchConfirm.value = { id, createCommandWorld };
        return;
    }
    if (!store.selectedAccount) {
        toast('请先在账户页选择或添加账号', 'error');
        store.currentView = 'accounts';
        return;
    }
    store.launchingVersionId = id;
    store.launchingFolder =
        currentVersion.value?.folder ??
        store.settings?.activeFolder ??
        store.settings?.gameDir ??
        '';
    store.launchState = { status: 'launching', text: '正在准备启动…' };
    try {
        await launchGame(id, undefined, currentVersion.value?.folder, createCommandWorld);
    } catch (error) {
        store.launchState = { status: 'error', text: errText(error) };
        toast('启动失败：' + errText(error), 'error');
    }
}

const restartBusy = ref(false);
const restartConfirm = ref<{ id: string; folder: string; token: string } | null>(null);
async function quickRestart(version: InstalledVersion, token?: string) {
    if (restartBusy.value) return;
    restartBusy.value = true;
    cardMenu.id = '';
    try {
        const folder = version.folder ?? store.settings?.activeFolder ?? '';
        const result = await restartGame(version.id, folder, token);
        restartConfirm.value = result.requiresForce
            ? { id: version.id, folder, token: result.forceToken! }
            : null;
        if (!result.requiresForce) toast('已确认退出并重新启动同一实例', 'success');
    } catch (e) {
        toast('重启失败：' + errText(e), 'error');
        restartConfirm.value = null;
    } finally {
        restartBusy.value = false;
    }
}
async function cancelRestartPrompt() {
    await cancelGameRestart();
    restartConfirm.value = null;
}

async function onLaunchClick() {
    if (launching.value || waitingWindow.value) return;
    // 移动端：未选实例不做静默 disabled，给出提示并直接带到版本管理
    if (!currentVersion.value && isAndroid) {
        toast('请先选择或下载一个游戏实例', 'info');
        openVersionSettings();
        return;
    }
    await startVersion(selectedId.value);
}

function openVersionSettings() {
    if (selectedId.value) localStorage.setItem(LAST_VERSION_KEY, selectedId.value);
    store.currentView = 'game';
}

const logOpen = ref(false);
const logBody = ref<HTMLElement | null>(null);
const exportingLogs = ref(false);

watch(
    () => store.logs.length,
    async () => {
        if (!logOpen.value) return;
        await nextTick();
        if (logBody.value) logBody.value.scrollTop = logBody.value.scrollHeight;
    }
);

async function exportFailureLogs() {
    if (exportingLogs.value) return;
    exportingLogs.value = true;
    try {
        const saved = await exportLaunchLogs(store.launchingVersionId || selectedId.value);
        if (saved) toast(`错误日志已导出：${saved}`, 'success');
    } catch (error) {
        toast(`导出失败：${errText(error)}`, 'error');
    } finally {
        exportingLogs.value = false;
    }
}

/** 分享 latest.log 到 mclo.gs：成功后复制链接到剪贴板 */
const sharingLogs = ref(false);
async function shareLogs() {
    if (sharingLogs.value) return;
    sharingLogs.value = true;
    try {
        const result = await shareLogsToMclogs(store.launchingVersionId || selectedId.value);
        await navigator.clipboard.writeText(result.url).catch(() => undefined);
        toast(`已上传 mclo.gs，链接已复制：${result.url}`, 'success');
    } catch (error) {
        toast(errText(error), 'error');
    } finally {
        sharingLogs.value = false;
    }
}

/** 导出最近一次启动的完整命令为 .bat（HMCL/PCL 同款排障利器） */
const exportingScript = ref(false);
async function exportScript() {
    if (exportingScript.value) return;
    exportingScript.value = true;
    try {
        const saved = await exportLaunchScript(
            selectedId.value,
            currentVersion.value?.folder
        );
        if (saved) toast(`启动脚本已导出：${saved}`, 'success');
    } catch (error) {
        toast(errText(error), 'error');
    } finally {
        exportingScript.value = false;
    }
}

// ---------------- Java 与内存摘要 ----------------
const javas = ref<JavaInfo[]>([]);
const javaChecked = ref(false);
const javaText = computed(() => {
    if (currentVersion.value?.javaAuto) return '自动选择';
    const versionJava =
        currentVersion.value?.javaPath ||
        (!store.settings?.javaAuto ? store.settings?.javaPath : '');
    if (versionJava) {
        const match = javas.value.find((java) => java.path === versionJava);
        return match
            ? `Java ${match.version} (${match.architecture ?? (match.is64Bit ? '64-bit' : '32-bit')})`
            : versionJava;
    }
    return '自动选择';
});
const javaPicker = ref<{ id: string; folder?: string; name: string; choice: string } | null>(null);
const javaSaving = ref(false);
function openJavaPicker() {
    const version = currentVersion.value;
    if (!version) {
        openSettings('java');
        return;
    }
    javaPicker.value = {
        id: version.id,
        folder: version.folder,
        name: versionLabel(version),
        choice: version.javaAuto ? '@auto' : version.javaPath || '@inherit'
    };
    void loadJavaSummaryImpl();
}
async function saveJavaChoice() {
    const target = javaPicker.value;
    if (!target || javaSaving.value) return;
    javaSaving.value = true;
    try {
        await setVersionJava(
            target.id,
            target.choice.startsWith('@') ? '' : target.choice,
            target.choice === '@auto',
            target.folder
        );
        await refreshInstalled();
        javaPicker.value = null;
        toast('已更新此实例的 Java 选择', 'success');
    } catch (error) {
        toast('保存失败：' + errText(error), 'error');
    } finally {
        javaSaving.value = false;
    }
}
const memoryText = computed(() => {
    // 与设置实时同步：开启自动分配显示「自动」，关闭显示手动数值
    if (store.settings?.memoryAuto === true) return '自动';
    const mb = store.settings?.memoryMB ?? 0;
    if (!mb) return '—';
    return mb % 1024 === 0 ? `${mb / 1024} GB` : `${(mb / 1024).toFixed(1)} GB`;
});

function loadJavaSummary() {
    return trackBootTask(loadJavaSummaryImpl, 800);
}
async function loadJavaSummaryImpl() {
    try {
        javas.value = await listJava();
    } catch {
        javas.value = [];
    } finally {
        javaChecked.value = true;
    }
}

// ---------------- 账户与 3D 皮肤 ----------------
const accountName = computed(() => store.selectedAccount?.username ?? '未登录');
const accountTypeLabel = computed(() => {
    const account = store.selectedAccount;
    if (!account) return '添加账户后开始游戏';
    if (account.type === 'microsoft') return 'Microsoft 正版账户';
    if (account.type === 'yggdrasil') return account.providerName ?? '外置 Yggdrasil';
    return '离线账户';
});
/** 外置登录卡的真实皮肤站图标（按 apiRoot/站名匹配官方 favicon，未收录回退字母） */
const providerIcon = computed(() =>
    store.selectedAccount?.type === 'yggdrasil'
        ? skinSiteIconUrl(store.selectedAccount.apiRoot, store.selectedAccount.providerName)
        : null
);

const skinProfile = ref<ProfileSkins | null>(null);
const skinLoading = ref(false);
const skinError = ref('');
let skinRequestToken = 0;

const currentSkin = computed(() => skinProfile.value?.skins[0] ?? null);
const skinSrc = computed(() => currentSkin.value?.dataUrl ?? '');
const skinVariant = computed<SkinVariant>(() =>
    currentSkin.value?.variant === 'slim' ? 'slim' : 'classic'
);
/** 首页 3D 预览与皮肤页共用披风渲染：有披风则显示，无则不显示（无手动开关） */
const activeCape = computed(() => skinProfile.value?.capes?.find((c) => c.active)?.dataUrl ?? '');

function reloadSkin(refresh = false) {
    return trackBootTask(() => reloadSkinImpl(refresh), 800);
}
async function reloadSkinImpl(refresh = false) {
    const request = ++skinRequestToken;
    skinError.value = '';
    if (!store.selectedAccount) {
        skinProfile.value = null;
        skinLoading.value = false;
        return;
    }
    skinLoading.value = true;
    try {
        const profile = await getSkinProfile(refresh);
        if (request === skinRequestToken) skinProfile.value = profile;
    } catch (error) {
        if (request === skinRequestToken) skinError.value = errText(error);
    } finally {
        if (request === skinRequestToken) skinLoading.value = false;
    }
}

watch(
    () => store.selectedAccount?.id,
    () => {
        skinProfile.value = null;
        void reloadSkin();
    }
);

// ---------------- 最近游戏与菜单 ----------------
// 收藏优先 + 最近游玩排序；启动某实例后 recordLastPlayed 更新使其自然提前。
// 选中实例不再直接置顶——只有启动过才排到第一个。
const wideRecent = ref(window.innerWidth >= 1500);
const updateRecentWidth = () => {
    wideRecent.value = window.innerWidth >= 1500;
};
onMounted(() => window.addEventListener('resize', updateRecentWidth));
onUnmounted(() => window.removeEventListener('resize', updateRecentWidth));
const recent = computed(() =>
    sortWithFavorite(activeInstalled.value).slice(0, wideRecent.value ? 8 : 4)
);
const sortedInstalled = computed(() => sortWithFavorite(store.installed));

const versionMenu = reactive({ open: false, top: 0, left: 0, width: 230 });
const folderListOpen = ref(false);
const folderSwitchBusy = ref(false);
async function chooseGameFolder(folder: string) {
    if (folderSwitchBusy.value) return;
    folderSwitchBusy.value = true;
    try {
        await setActiveFolder(folder);
        store.settings = await getSettings();
        store.resourceVersionId = '';
        await refreshInstalled();
        folderListOpen.value = false;
    } catch (error) {
        toast(errText(error), 'error');
    } finally {
        folderSwitchBusy.value = false;
    }
}
const versionMenuButton = ref<HTMLElement | null>(null);

function toggleVersionMenu() {
    if (!versionMenu.open && versionMenuButton.value) {
        const bounds = versionMenuButton.value.getBoundingClientRect();
        versionMenu.top = Math.min(window.innerHeight - 300, bounds.bottom + 8);
        versionMenu.left = Math.max(8, Math.min(window.innerWidth - 250, bounds.right - 230));
    }
    versionMenu.open = !versionMenu.open;
    // 打开后把焦点移入浮层容器，↑↓/ESC 立即可用（键盘契约对齐 AppMenu）
    if (versionMenu.open) void nextTick(() => versionMenuEl.value?.focus({ preventScroll: true }));
}

function chooseVersion(id: string) {
    selectedId.value = id;
    versionMenu.open = false;
}

/** 实例选择浮层键盘支持（与 AppMenu/SelectMenu 同一套纪律）：↑↓/Home/End 移动、Enter 选中、ESC/Tab 关闭并归还焦点 */
const versionMenuEl = ref<HTMLElement | null>(null);
function onVersionMenuKey(event: KeyboardEvent) {
    const items = [
        ...(versionMenuEl.value?.querySelectorAll<HTMLButtonElement>(
            '.menu-item:not(:disabled)'
        ) || [])
    ];
    if (event.key === 'Escape') {
        event.preventDefault();
        event.stopImmediatePropagation();
        versionMenu.open = false;
        versionMenuButton.value?.focus({ preventScroll: true });
        return;
    }
    if (event.key === 'Tab') {
        versionMenu.open = false;
        return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    if (!items.length) return;
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
        event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? items.length - 1
              : index < 0
                ? event.key === 'ArrowDown'
                  ? 0
                  : items.length - 1
                : Math.max(0, Math.min(items.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    items[next].focus();
}

// ---------------- 实例卡 ⋯ 菜单（AppMenu：去重 + 分组；启动实例与卡片 play 按钮重复，删除） ----------------
const cardMenu = reactive({ id: '', anchor: null as HTMLElement | null });
const cardMenuVersion = computed(() =>
    store.installed.find((version) => version.id === cardMenu.id)
);

function openCardMenu(event: MouseEvent, id: string) {
    // 再点同一颗 ⋯ 收起；定位/翻转/点击外部关闭交给 AppMenu
    if (cardMenu.id === id) {
        cardMenu.id = '';
        cardMenu.anchor = null;
        return;
    }
    cardMenu.anchor = event.currentTarget as HTMLElement;
    cardMenu.id = id;
}

/** 分组：实例管理 → 启动与排障 → 危险区（对齐 GameView 实例 ⋯ 菜单的三段式） */
const cardMenuItems = computed<AppMenuItem[]>(() => {
    const v = cardMenuVersion.value;
    if (!v) return [];
    return [
        { id: 'manage', label: '管理实例', group: '' },
        {
            id: 'favorite',
            label: isFavorite(v.id, v.folder) ? '取消收藏' : '收藏实例',
            group: ''
        },
        { id: 'folder', label: '打开文件夹', group: '' },
        {
            id: 'cmdworld',
            label: '启动并创建命令世界',
            group: '启动与排障',
            disabled: running.value || launching.value || waitingWindow.value || restartBusy.value,
            disabledReason: '有实例正在启动或运行，结束后再试'
        },
        {
            id: 'restart',
            label: '快速重启游戏',
            group: '启动与排障',
            disabled: !running.value || restartBusy.value,
            disabledReason: running.value ? undefined : '游戏未在运行，无需重启'
        },
        { id: 'script', label: '导出启动脚本（.bat）', group: '启动与排障' },
        { id: 'remove', label: '删除实例', group: '危险区', danger: true }
    ];
});

function onCardMenuSelect(id: string) {
    const v = cardMenuVersion.value;
    if (!v) return;
    if (id === 'manage') openInstanceCenter(v);
    else if (id === 'favorite') toggleFavorite(v.id, v.folder);
    else if (id === 'folder') {
        void openVersionFolder(v.id);
        return; // openVersionFolder / requestRemove 内部已收起菜单
    } else if (id === 'cmdworld') void startVersion(v.id, true);
    else if (id === 'restart') void quickRestart(v);
    else if (id === 'script') void exportScript();
    else if (id === 'remove') {
        requestRemove(v);
        return;
    }
    cardMenu.id = '';
    cardMenu.anchor = null;
}

async function openVersionFolder(id: string) {
    cardMenu.id = '';
    try {
        await openDir(`versions/${id}`);
    } catch (error) {
        toast('打开文件夹失败：' + errText(error), 'error');
    }
}

const removeModal = reactive({
    open: false,
    target: null as InstalledVersion | null,
    busy: false
});

function requestRemove(version: InstalledVersion) {
    cardMenu.id = '';
    removeModal.target = version;
    removeModal.open = true;
}

async function confirmRemove() {
    const version = removeModal.target;
    if (!version || removeModal.busy) return;
    removeModal.busy = true;
    try {
        await removeVersion(version.id, version.folder);
        await refreshInstalled();
        removeModal.open = false;
        toast(`已删除 ${version.id}`, 'success');
    } catch (error) {
        toast('删除失败：' + errText(error), 'error');
    } finally {
        removeModal.busy = false;
    }
}

onMounted(() => {
    startBannerTimer();
    void loadJavaSummary();
    void reloadSkin();
});

onUnmounted(() => {
    stopBannerTimer();
    readyBanners.clear();
    skinRequestToken++;
});
</script>

<template>
    <!-- ================= Android 启动台（移动端专属信息架构，参照 ZL2/FCL：
         一屏一个主题：当前实例 + 启动 + 账号；其余功能各归各屏，不做桌面布局压缩） ================= -->
    <div v-if="isAndroid" data-ui="HomeView:m000launchpad" class="m-launchpad">
        <!-- 中：舞台 = 桌面首页横幅卡的移动化（背景图 + 版本元素 + 两角操作） -->
        <section
            data-ui="HomeView:m005stage"
            class="m-stage"
            :class="{
                'has-bg': banners.length || showHeroAnimation,
                'no-version': !currentVersion
            }"
            @mouseenter="heroHovered = true"
            @mouseleave="heroHovered = false"
        >
            <HeroSceneCanvas
                v-if="showHeroAnimation"
                data-ui="HomeView:m005scene"
                compact
                :hover-paused="heroHovered"
                :flare-target="launchHovered"
                :loading="heroLoading"
                :dimmed="heroDimmed"
                :instance-key="heroInstanceKey"
                @fallback="onHeroFallback"
            />
            <img
                data-ui="HomeView:m024bg"
                v-if="!showHeroAnimation && banners.length"
                class="m-stage-bg"
                :src="banners[0]?.src"
                alt=""
            />
            <div
                data-ui="HomeView:m025shade"
                v-if="banners.length || showHeroAnimation"
                class="m-stage-shade"
            ></div>

            <!-- 左上：当前版本 + 框起的版本名元素 -->
            <div data-ui="HomeView:m006head" class="m-stage-head">
                <span data-ui="HomeView:m007kicker" class="m-kicker">当前版本</span>
                <button
                    data-ui="HomeView:m008namechip"
                    class="m-namechip"
                    @click="openVersionSettings"
                >
                    {{ currentVersion ? heroName : '尚未选择实例' }}
                </button>
            </div>

            <!-- 右上：运行状态 pill（一行紧凑） -->
            <span
                data-ui="HomeView:m018status"
                class="m-status"
                :class="`is-${heroStatus.tone}`"
            >
                <i></i>{{ heroStatus.text }} · {{ javaText }} · {{ memoryText }}
            </span>

            <!-- 左下：版本设置 / 右下：更改版本 -->
            <div data-ui="HomeView:m040corner" class="m-stage-corner left">
                <button
                    data-ui="HomeView:m041settings"
                    class="m-corner-btn"
                    @click="openVersionSettings"
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06-.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06-.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
                    </svg>
                    版本设置
                </button>
            </div>
            <div data-ui="HomeView:m042corner" class="m-stage-corner right">
                <button
                    data-ui="HomeView:m043change"
                    class="m-corner-btn"
                    @click="openVersionSettings"
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
                        <path d="M3 11h18" />
                    </svg>
                    更改版本
                </button>
            </div>
        </section>

        <!-- 右：操作坞（账号卡 → 皮肤预览 → 启动） -->
        <aside data-ui="HomeView:m030dock" class="m-dock">
            <button
                data-ui="HomeView:m036account"
                class="m-account"
                @click="store.currentView = 'accounts'"
            >
                <Avatar :size="34" />
                <span data-ui="HomeView:m037accinfo" class="m-account-info">
                    <b>{{ accountName }}</b>
                    <i>{{
                        store.selectedAccount
                            ? store.selectedAccount.type === 'offline'
                                ? '离线账号'
                                : '已登录'
                            : '点击添加或选择账户'
                    }}</i>
                </span>
            </button>

            <div
                data-ui="HomeView:m031skin"
                class="m-skin"
                @click="store.currentView = 'skins'"
            >
                <div data-ui="HomeView:m033stage2" class="m-skin-stage">
                    <SkinViewer3D :src="skinSrc" :variant="skinVariant" :cape="activeCape" />
                    <div data-ui="HomeView:m034loading" v-if="skinLoading" class="m-skin-overlay">
                        <span class="spin"></span>
                    </div>
                </div>
                <span data-ui="HomeView:m038hint" class="m-skin-hint">
                    {{ currentSkin ? (skinVariant === 'slim' ? '纤细模型' : '经典模型') : '动态角色' }}
                    · 拖动旋转 · 点击编辑
                </span>
            </div>

            <button
                data-ui="HomeView:m011launch"
                class="m-launch"
                :class="{ launching }"
                @click="onLaunchClick"
                @mouseenter="launchHovered = true"
                @mouseleave="launchHovered = false"
            >
                <span
                    data-ui="HomeView:m012progress"
                    v-if="launching"
                    class="launch-progress"
                    :style="{ width: percent + '%' }"
                ></span>
                <!-- 手绘播放三角 SVG 收编 UiGlyph play（inventory H） -->
                <UiGlyph
                    data-ui="HomeView:m013icon"
                    name="play"
                    :size="20"
                    filled
                />
                <span data-ui="HomeView:m014text">{{
                    currentVersion ? launchText : '选择实例启动'
                }}</span>
            </button>
        </aside>
    </div>

    <!-- ================= 桌面端原布局（Android 不渲染，互不影响） ================= -->
    <div v-else data-ui="HomeView:a32c099bc136" class="home-dashboard">
        <div data-ui="HomeView:5fc354f9fa6d" class="home-main">
            <section
                data-ui="HomeView:53555cbc5ab3"
                class="hero-card"
                :class="{ 'no-banner': !banners.length && !showHeroAnimation }"
                data-edit="banner"
                @mouseenter="heroHovered = true"
                @mouseleave="heroHovered = false"
            >
                <HeroSceneCanvas
                    v-if="showHeroAnimation"
                    data-ui="HomeView:heroScene"
                    :hover-paused="heroHovered"
                    :flare-target="launchHovered"
                    :loading="heroLoading"
                    :dimmed="heroDimmed"
                    :instance-key="heroInstanceKey"
                    :speed="heroAnimationSpeed"
                    :dwell-seconds="heroDwellSeconds"
                    @scene="heroSceneInfo = $event"
                    @fallback="onHeroFallback"
                />
                <template v-if="!showHeroAnimation">
                    <img
                        data-ui="HomeView:1182a4262184"
                        v-for="item in bannerLayers"
                        :key="item.path"
                        :src="item.src"
                        class="hero-image"
                        :class="{ active: item.index === bannerIndex }"
                        :style="{ objectFit: item.fit }"
                        alt=""
                        aria-hidden="true"
                        @error="onBannerError(item)"
                        @load="readyBanners.add(item.src)"
                        @transitionend="finishBannerExit(item.index, $event)"
                    />
                </template>
                <div
                    data-ui="HomeView:5838d59b9e2a"
                    v-if="banners.length || showHeroAnimation"
                    class="hero-shade"
                ></div>

                <div data-ui="HomeView:2e850cf13849" class="hero-content" data-edit="bannerText">
                    <span data-ui="HomeView:13616e708e66" class="hero-kicker">当前版本</span>
                    <div data-ui="HomeView:027bc7e292dc" class="hero-metadata-slot">
                        <Transition name="instance-switch" mode="out-in">
                            <div
                                data-ui="HomeView:e8ce122328e6"
                                :key="
                                    JSON.stringify([
                                        currentVersion?.folder,
                                        selectedId,
                                        heroName,
                                        heroVersion,
                                        currentVersion?.loader,
                                        currentVersion?.loaderVersion
                                    ])
                                "
                                class="hero-metadata"
                            >
                                <h1
                                    data-ui="HomeView:d209b16cd00e"
                                    :title="heroName"
                                    :class="{ 'long-name': heroName.length > 16 }"
                                >
                                    {{ heroName }}
                                </h1>
                                <div data-ui="HomeView:273ef3354188" class="hero-edition">
                                    <span
                                        data-ui="HomeView:cbc929d650f0"
                                        v-if="currentVersion && !heroName.includes(heroVersion)"
                                        class="hero-game-version"
                                        :title="`Minecraft ${heroVersion}`"
                                        >{{ heroVersion }}</span
                                    >
                                    <span
                                        data-ui="HomeView:323afa8d269b"
                                        v-if="
                                            currentVersion &&
                                            !heroName.includes(loaderText(currentVersion))
                                        "
                                        class="loader-badge"
                                        >{{ loaderText(currentVersion) }}</span
                                    >
                                </div>
                            </div>
                        </Transition>
                    </div>

                    <!-- 像素动画场景文案块（色点 + 眉标 + 标题 + 注），随场景切换淡入 -->
                    <Transition v-if="showHeroAnimation && heroSceneInfo" name="hero-scene-copy" mode="out-in">
                        <div :key="heroSceneInfo.id" class="hero-scene-copy">
                            <span class="hero-scene-eyebrow">
                                <i :style="{ background: heroSceneInfo.accent }"></i>{{ heroSceneInfo.eyebrow }}
                            </span>
                            <strong class="hero-scene-title">{{ heroSceneInfo.title }}</strong>
                            <span class="hero-scene-caption">{{ heroSceneInfo.caption
                            }}<em v-if="heroSceneInfo.note"> · {{ heroSceneInfo.note }}</em></span>
                        </div>
                    </Transition>

                    <div data-ui="HomeView:b9457a1e78a6" class="hero-actions">
                        <div data-ui="HomeView:8079f9c9866a" class="hero-secondary-actions">
                            <button
                                data-ui="HomeView:f170f5154efb"
                                class="hero-settings"
                                :disabled="!currentVersion"
                                @click="openVersionSettings"
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="1.7"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <circle cx="12" cy="12" r="3" />
                                    <path
                                        d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1.4 1.68V21h-4v-.08A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15 1.7 1.7 0 0 0 3 13.6H3v-4h.08A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10.4 3H14a1.7 1.7 0 0 0 1.4 1.6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9 1.7 1.7 0 0 0 21 10.4V14a1.7 1.7 0 0 0-1.6 1Z"
                                    />
                                </svg>
                                版本设置
                            </button>
                            <button
                                data-ui="HomeView:8389ad960143"
                                class="hero-more"
                                :disabled="!currentVersion"
                                title="更多实例操作"
                                @click="currentVersion && openCardMenu($event, currentVersion.id)"
                            >
                                <!-- 手绘三点 SVG 收编 UiGlyph more（inventory H） -->
                                <UiGlyph name="more" :size="19" />
                            </button>
                        </div>

                        <div
                            data-ui="HomeView:27feb55000dd"
                            class="launch-combo"
                            data-edit="accent"
                        >
                            <button
                                data-ui="HomeView:3fed5eb9ac9e"
                                class="launch-main"
                                :class="{ launching }"
                                :disabled="launching || waitingWindow || (!currentVersion && !isAndroid)"
                                @click="onLaunchClick"
                                @mouseenter="launchHovered = true"
                                @mouseleave="launchHovered = false"
                            >
                                <span
                                    data-ui="HomeView:62395e18f043"
                                    v-if="launching"
                                    class="launch-progress"
                                    :style="{ width: percent + '%' }"
                                ></span>
                                <span data-ui="HomeView:57cbf5d7ee2f" class="launch-content">
                                    <!-- 手绘播放三角 SVG 收编 UiGlyph play（inventory H） -->
                                    <UiGlyph name="play" :size="22" filled />
                                    <span>{{ launchText }}</span>
                                </span>
                            </button>
                            <button
                                data-ui="HomeView:5ca10c2a5c92"
                                ref="versionMenuButton"
                                class="launch-arrow"
                                title="选择游戏实例"
                                @click="toggleVersionMenu"
                            >
                                <svg
                                    :class="{ open: versionMenu.open }"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="1.7"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="m6 9 6 6 6-6" />
                                </svg>
                            </button>
                        </div>
                    </div>
                </div>
            </section>

            <section
                data-ui="HomeView:a9a31ca336dd"
                ref="runtimeStrip"
                class="runtime-strip"
                data-edit="card"
                @mouseleave="runtimeHover = -1"
            >
                <span
                    data-ui="HomeView:43baf1da9a9d"
                    class="runtime-blob"
                    :class="{ on: runtimeHover >= 0 }"
                    :style="runtimeBlobStyle"
                    aria-hidden="true"
                ></span>
                <button
                    data-ui="HomeView:5d665ee0e06b"
                    class="runtime-item"
                    @mouseenter="runtimeHover = 0"
                    @click="openJavaPicker"
                    title="选择此实例的 Java：自动或手动"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path
                            d="M8 2v4M16 2v4M7 8h10a4 4 0 0 1 4 4v0a8 8 0 0 1-8 8h-2a8 8 0 0 1-8-8v0a4 4 0 0 1 4-4Z"
                        />
                        <path d="M8 13h8M9 17h6" />
                    </svg>
                    <span
                        ><small>运行环境</small><strong>{{ javaText }}</strong></span
                    >
                    <!-- 手绘箭头 SVG 收编 UiGlyph chevron-right（inventory H 重复手绘 SVG 组） -->
                    <UiGlyph class="runtime-chevron" name="chevron-right" :size="14" />
                </button>
                <button
                    data-ui="HomeView:ac285743c8c3"
                    class="runtime-item"
                    @mouseenter="runtimeHover = 1"
                    @click="openSettings('memory')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <rect x="5" y="5" width="14" height="14" rx="2" />
                        <path
                            d="M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4M9 9h6v6H9Z"
                        />
                    </svg>
                    <span
                        ><small>内存分配</small><strong>{{ memoryText }}</strong></span
                    >
                    <!-- 手绘箭头 SVG 收编 UiGlyph chevron-right（inventory H 重复手绘 SVG 组） -->
                    <UiGlyph class="runtime-chevron" name="chevron-right" :size="14" />
                </button>
                <button
                    data-ui="HomeView:3e2f60ad69a5"
                    class="runtime-item runtime-state"
                    :class="heroStatus.tone"
                    @mouseenter="runtimeHover = 2"
                    @click="logOpen = true"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M3 12h4l2-7 4 14 2-7h6" />
                    </svg>
                    <span
                        ><small>运行状态</small><strong><i></i>{{ heroStatus.text }}</strong></span
                    >
                    <!-- 手绘箭头 SVG 收编 UiGlyph chevron-right（inventory H 重复手绘 SVG 组） -->
                    <UiGlyph class="runtime-chevron" name="chevron-right" :size="14" />
                </button>
            </section>

            <section data-ui="HomeView:d4eaa1c0d798" class="instances-block">
                <div data-ui="HomeView:07c360f67ff2" class="instances-head">
                    <h2 data-ui="HomeView:ab67b2084be9">最近游戏</h2>
                    <button
                        data-ui="HomeView:1f3a4e95599d"
                        class="manage-instances"
                        @click="store.currentView = 'game'"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="1.7"
                        >
                            <rect x="3" y="3" width="7" height="7" rx="1" />
                            <rect x="14" y="3" width="7" height="7" rx="1" />
                            <rect x="3" y="14" width="7" height="7" rx="1" />
                            <rect x="14" y="14" width="7" height="7" rx="1" />
                        </svg>
                        管理实例
                    </button>
                </div>

                <div
                    data-ui="HomeView:204c17c101ec"
                    v-if="store.installed.length"
                    class="instance-grid"
                >
                    <article
                        data-ui="HomeView:22d3ec196f2b"
                        v-for="version in recent"
                        :key="version.id"
                        class="instance-card"
                        tabindex="0"
                        @keydown.enter.self="chooseVersion(version.id)"
                        @keydown.space.self.prevent="chooseVersion(version.id)"
                        :class="{ selected: version.id === selectedId }"
                        data-edit="card"
                        @click="chooseVersion(version.id)"
                        @contextmenu.prevent="showFolderContextMenu(version.folder, version.id)"
                    >
                        <img
                            data-ui="HomeView:ead234e5a835"
                            v-if="versionIconUrl(version)"
                            class="instance-icon image"
                            :src="versionIconUrl(version)"
                            alt=""
                        />
                        <svg v-else class="instance-icon" viewBox="0 0 48 48" aria-hidden="true">
                            <polygon points="24,5 43,14.5 24,24 5,14.5" fill="#79c144" />
                            <polygon points="5,14.5 24,24 24,29.5 5,20" fill="#5da236" />
                            <polygon points="24,24 43,14.5 43,20 24,29.5" fill="#4e8a2f" />
                            <polygon points="5,20 24,29.5 24,43 5,33.5" fill="#8b5e34" />
                            <polygon points="24,29.5 43,20 43,33.5 24,43" fill="#6f4a29" />
                        </svg>
                        <div data-ui="HomeView:0541baba1a35" class="instance-copy">
                            <strong
                                data-ui="HomeView:51623bcd9cd5"
                                :title="versionLabel(version)"
                                >{{ versionLabel(version) }}</strong
                            >
                            <span
                                data-ui="HomeView:59cc23bdfbdc"
                                :title="displayVersionSub(version)"
                                >{{ displayVersionSub(version) }}</span
                            >
                        </div>
                        <button
                            data-ui="HomeView:b5167161777c"
                            class="instance-more"
                            title="更多"
                            @click.stop="openCardMenu($event, version.id)"
                        >
                            <!-- 手绘三点 SVG 收编 UiGlyph more（inventory H） -->
                            <UiGlyph name="more" :size="15" />
                        </button>
                        <span
                            data-ui="HomeView:ba095d8dcc15"
                            v-if="store.lastPlayed[version.id]"
                            class="instance-last"
                            >上次游玩：{{ fmtLastPlayed(store.lastPlayed[version.id]) }}</span
                        >
                        <button
                            data-ui="HomeView:1a638472fc2a"
                            class="instance-play"
                            :disabled="launching"
                            :title="`启动 ${version.id}`"
                            @click.stop="startVersion(version.id)"
                        >
                            <!-- 手绘播放三角 SVG 收编 UiGlyph play（inventory H） -->
                            <UiGlyph name="play" :size="15" filled />
                        </button>
                    </article>
                </div>
                <!-- 空态收编 EmptyState（UX「空状态指引」）：原整块按钮改为说明 + 明确入口按钮；
                     data-ui 锚点随元素迁移（旧 c485ad83d7d3 在空态按钮上，新锚点落容器） -->
                <EmptyState
                    v-else
                    data-ui="HomeView:c485ad83d7d3"
                    icon="cube"
                    title="还没有游戏实例"
                    description="去版本管理下载或导入一个实例，这里就会显示最近游玩"
                >
                    <template #action>
                        <button class="btn btn-gold btn-sm" @click="store.currentView = 'game'">
                            前往版本管理
                        </button>
                    </template>
                </EmptyState>
            </section>
        </div>

        <aside data-ui="HomeView:3ce39fef1300" class="home-side">
            <section data-ui="HomeView:1a033831e623" class="account-panel" data-edit="card">
                <template v-if="store.selectedAccount">
                    <div class="account-head">
                        <Avatar :size="54" />
                        <div data-ui="HomeView:69c3212000eb" class="account-copy" data-edit="text">
                            <strong>{{ accountName }}</strong>
                            <span
                                ><i></i
                                >{{
                                    store.selectedAccount.type === 'offline' ? '离线账号' : '已登录'
                                }}</span
                            >
                        </div>
                        <button
                            data-ui="HomeView:c7c1ffb68ac7"
                            class="account-more"
                            title="账户管理"
                            @click="store.currentView = 'accounts'"
                        >
                            <UiGlyph name="more" :size="17" />
                        </button>
                    </div>
                    <button class="account-provider" @click="store.currentView = 'accounts'">
                        <span
                            data-ui="HomeView:8bfe0faab45b"
                            class="provider-mark"
                            :class="store.selectedAccount.type"
                            ><BrandMicrosoft
                                v-if="store.selectedAccount.type === 'microsoft'"
                            /><img
                                v-else-if="providerIcon"
                                :src="providerIcon"
                                class="provider-logo"
                                :alt="accountTypeLabel"
                            /><template v-else>{{
                                store.selectedAccount.type === 'yggdrasil' ? 'Y' : 'O'
                            }}</template></span
                        >
                        <span>{{ accountTypeLabel }}</span>
                        <UiGlyph name="chevron-right" :size="15" />
                    </button>
                </template>
                <template v-else>
                    <div class="account-head">
                        <div data-ui="HomeView:14b73e10a66b" class="account-placeholder">?</div>
                        <div data-ui="HomeView:b713dda14ce9" class="account-copy">
                            <strong>未登录</strong
                            ><span data-ui="HomeView:ab6dcd8df3ac" class="offline-state"
                                >请选择账户</span
                            >
                        </div>
                    </div>
                    <button class="account-provider" @click="store.currentView = 'accounts'">
                        <span data-ui="HomeView:57ae15324d65" class="provider-mark offline">+</span
                        ><span>添加或选择账户</span>
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                        >
                            <path d="m9 6 6 6-6 6" />
                        </svg>
                    </button>
                </template>
            </section>

            <section data-ui="HomeView:f56ae81d7b8f" class="skin-panel" data-edit="card">
                <div data-ui="HomeView:a11abb820a96" class="skin-head">
                    <div>
                        <h3>皮肤预览</h3>
                        <span>{{
                            currentSkin
                                ? skinVariant === 'slim'
                                    ? '纤细模型'
                                    : '经典模型'
                                : '动态角色'
                        }}</span>
                    </div>
                    <button
                        data-ui="HomeView:d5082937e164"
                        class="skin-refresh"
                        :disabled="skinLoading || !store.selectedAccount"
                        title="联网刷新皮肤（默认使用本地缓存）"
                        @click="reloadSkin(true)"
                    >
                        <svg
                            :class="{ spinning: skinLoading }"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="1.7"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path d="M20 7v5h-5" />
                            <path d="M4 17v-5h5" />
                            <path d="M6.1 9A7 7 0 0 1 18 6l2 1M4 17l2 1a7 7 0 0 0 11.9-3" />
                        </svg>
                    </button>
                </div>
                <div
                    data-ui="HomeView:22e64941359b"
                    class="skin-stage"
                    @dblclick="store.currentView = store.selectedAccount ? 'skins' : 'accounts'"
                >
                    <SkinViewer3D :src="skinSrc" :variant="skinVariant" :cape="activeCape" />
                    <div data-ui="HomeView:862dfc8e4099" v-if="skinLoading" class="skin-overlay">
                        <span data-ui="HomeView:369e7ffcf786" class="spin"></span
                        ><span>正在加载皮肤…</span>
                    </div>
                    <button
                        data-ui="HomeView:70bc857e7889"
                        v-else-if="store.selectedAccount && skinError"
                        class="skin-overlay action error"
                        :title="skinError"
                        @click="reloadSkin(true)"
                    >
                        皮肤加载失败，点击重试
                    </button>
                </div>
                <button
                    data-ui="HomeView:d3685d94fd0f"
                    class="skin-tip"
                    @click="store.currentView = store.selectedAccount ? 'skins' : 'accounts'"
                >
                    拖动可旋转 · 行走动画
                    <UiGlyph name="chevron-right" :size="12" />
                </button>
            </section>
            <CreatorCard class="home-creator" />
            <LaunchHeatmap class="card home-heatmap" />
        </aside>

        <Teleport to="body">
            <div
                data-ui="HomeView:d64e8ccae3be"
                v-if="versionMenu.open"
                class="menu-overlay"
                @click="versionMenu.open = false"
            ></div>
            <!-- 版本/文件夹选择器（「选择器」而非动作菜单：两层列表 + active 标记 + 行右键管理，
                 不套 AppMenu——AppMenu 契约是纯动作列表；保留 float-menu 形态，
                 键盘契约对齐 AppMenu/SelectMenu：↑↓/Home/End 移动、Enter 选中、ESC/Tab 关闭归还焦点） -->
            <div
                data-ui="HomeView:d4dfd8dd87c8"
                v-if="versionMenu.open"
                ref="versionMenuEl"
                class="float-menu"
                tabindex="-1"
                :style="{
                    top: versionMenu.top + 'px',
                    left: versionMenu.left + 'px',
                    width: versionMenu.width + 'px'
                }"
                @keydown="onVersionMenuKey"
            >
                <button
                    data-ui="HomeView:9fae1e3ba335"
                    class="menu-item"
                    @click="folderListOpen = !folderListOpen"
                >
                    <template v-if="folderListOpen"
                        ><UiGlyph name="chevron-left" :size="12" /> 返回版本选择</template
                    >
                    <template v-else
                        >文件夹列表 <UiGlyph name="chevron-right" :size="12"
                    /></template>
                </button>
                <template v-if="folderListOpen">
                    <button
                        data-ui="HomeView:4aa05f9d0808"
                        v-for="folder in store.settings?.folders || []"
                        :key="folder.path"
                        class="menu-item"
                        :class="{ active: folder.path === store.settings?.activeFolder }"
                        :disabled="folderSwitchBusy"
                        :title="folder.path"
                        @click="chooseGameFolder(folder.path)"
                        @contextmenu.prevent="showFolderContextMenu(folder.path)"
                    >
                        {{ folder.name }}
                    </button>
                    <button
                        data-ui="HomeView:a82260e8c53c"
                        class="menu-item"
                        @click="store.currentView = 'game'"
                    >
                        添加 / 管理文件夹
                    </button>
                </template>
                <template v-else>
                    <button
                        data-ui="HomeView:dedcb316a30b"
                        v-for="version in sortedInstalled"
                        :key="version.id"
                        class="menu-item"
                        :class="{ active: version.id === selectedId }"
                        @click="chooseVersion(version.id)"
                        @contextmenu.prevent="showFolderContextMenu(version.folder, version.id)"
                    >
                        <!-- 手绘星形 SVG 收编 UiGlyph star（inventory H） -->
                        <UiGlyph
                            v-if="isFavorite(version.id, version.folder)"
                            name="star"
                            :size="12"
                            filled
                        />
                        <span data-ui="HomeView:f77a89b73779" v-else class="menu-spacer"></span>
                        {{ versionLabel(version) }}
                    </button>
                    <div
                        data-ui="HomeView:2de4cc34bb59"
                        v-if="!sortedInstalled.length"
                        class="menu-empty"
                    >
                        暂无已安装实例
                    </div>
                </template>
            </div>
        </Teleport>

        <!-- 实例卡 ⋯ 菜单收编 AppMenu（UX「复用现有 UI」）：定位/翻转/点击外部/键盘/焦点归还托管。
             去重：「启动实例」与卡片 play 按钮重复 → 删除（任务书拍板）；
             分组：实例管理 → 启动与排障 → 危险区（删除实例 danger）。 -->
        <AppMenu
            :open="!!cardMenu.id"
            :anchor="cardMenu.anchor"
            :items="cardMenuItems"
            :groups="['', '启动与排障', '危险区']"
            align="end"
            :min-width="230"
            aria-label="实例操作"
            @close="
                cardMenu.id = '';
                cardMenu.anchor = null;
            "
            @select="onCardMenuSelect"
        />

        <!-- 启动日志弹窗收编 AppModal：遮罩/标题/焦点陷阱/ESC 由全局体系托管（修复旧弹窗无 ESC）；
             业务内容（失败条 / 日志体 / 清空按钮）原样保留。宽屏档位依赖全局 .modal--xl（见报告）。 -->
        <AppModal :open="logOpen" title="启动日志" size="xl" @close="logOpen = false">
            <p class="log-subtitle">{{ store.logs.length }} 行 · {{ heroStatus.text }}</p>
            <div data-ui="HomeView:1921d3d812a6" v-if="launchFailed" class="log-failure">
                <span>检测到启动失败或异常退出</span>
                <span style="display: flex; gap: 8px">
                    <button
                        data-ui="HomeView:4e02813325fa"
                        :disabled="exportingLogs"
                        @click="exportFailureLogs"
                    >
                        {{ exportingLogs ? '导出中…' : '导出错误日志' }}
                    </button>
                    <button
                        class="btn btn-ghost"
                        :disabled="sharingLogs"
                        title="脱敏后上传 mclo.gs 并复制链接，方便发给别人帮你看"
                        @click="shareLogs"
                    >
                        {{ sharingLogs ? '上传中…' : '分享到 mclo.gs' }}
                    </button>
                </span>
            </div>
            <div data-ui="HomeView:d816343842fa" ref="logBody" class="log-body">
                <p data-ui="HomeView:9e34f7306c51" v-if="!store.logs.length" class="log-empty">
                    暂无启动日志
                </p>
                <pre
                    data-ui="HomeView:b8671047aacf"
                    v-else
                ><span data-ui="HomeView:d2bfb16eae17" v-for="(line, index) in store.logs" :key="index">{{ line }}</span></pre>
            </div>
            <template #actions>
                <button
                    data-ui="HomeView:287dd6a53ff1"
                    class="btn btn-ghost btn-sm"
                    :disabled="!store.logs.length"
                    @click="store.logs = []"
                >
                    清空日志
                </button>
            </template>
        </AppModal>

        <ConfirmModal
            :open="removeModal.open"
            title="删除版本"
            :message="`确定要删除版本「${removeModal.target?.id}」吗？该版本目录将移入系统回收站（共享依赖与资源保留）。`"
            :busy="removeModal.busy"
            @cancel="removeModal.open = false"
            @confirm="confirmRemove"
        />
    </div>
    <!-- Java 选择器收编 AppModal：遮罩点击/ESC 关闭（保存中 busy 不关）、焦点陷阱由 modalFocus 托管；
         radio 选择业务原样保留。旧宽 580px 待全局 .modal--lg 档位（见报告）。 -->
    <AppModal
        :open="!!javaPicker"
        title="选择 Java 运行环境"
        size="lg"
        :busy="javaSaving"
        @close="javaPicker = null"
    >
        <template v-if="javaPicker">
            <p data-ui="HomeView:96c5153e7b82" class="java-picker-description">
                {{ javaPicker.name }} · 仅修改此实例，不影响其他实例
            </p>
            <label class="java-option"
                ><input
                    data-ui="HomeView:e530b03f8660"
                    v-model="javaPicker.choice"
                    type="radio"
                    value="@auto"
                    name="home-java"
                /><span
                    ><strong>自动选择</strong
                    ><small>按游戏的真实版本要求匹配 Java，必要时自动下载</small></span
                ></label
            >
            <label class="java-option"
                ><input
                    data-ui="HomeView:a0b100241102"
                    v-model="javaPicker.choice"
                    type="radio"
                    value="@inherit"
                    name="home-java"
                /><span
                    ><strong>跟随全局设置</strong
                    ><small>{{
                        store.settings?.javaAuto
                            ? '当前全局：自动选择'
                            : '当前全局：' + (store.settings?.javaPath || '匹配本地 Java')
                    }}</small></span
                ></label
            >
            <div data-ui="HomeView:2b732ea02a21" class="java-list">
                <label
                    data-ui="HomeView:484d96932255"
                    v-for="java in javas"
                    :key="java.path"
                    class="java-option"
                    ><input
                        data-ui="HomeView:3b272633f5d0"
                        v-model="javaPicker.choice"
                        type="radio"
                        :value="java.path"
                        name="home-java"
                    /><span
                        ><strong
                            >Java {{ java.version }} ·
                            {{ java.architecture || (java.is64Bit ? '64-bit' : '32-bit') }}</strong
                        ><small data-ui="HomeView:012aa6cd6faf" :title="java.path">{{
                            java.path
                        }}</small></span
                    ></label
                >
                <p
                    data-ui="HomeView:cf5a93492c1f"
                    v-if="!javas.length"
                    class="java-picker-description"
                >
                    {{
                        javaChecked
                            ? '未发现本地 Java，可使用自动选择，或在设置中添加 Java。'
                            : '正在扫描本地 Java…'
                    }}
                </p>
                <p
                    data-ui="HomeView:c23c056bc1bd"
                    v-if="
                        javaPicker.choice &&
                        !javaPicker.choice.startsWith('@') &&
                        !javas.some((java) => java.path === javaPicker?.choice)
                    "
                    class="java-picker-description"
                >
                    当前指定：{{ javaPicker.choice }}
                </p>
            </div>
        </template>
        <template #actions>
            <button
                data-ui="HomeView:765c5489bdca"
                class="btn btn-ghost"
                :disabled="javaSaving"
                @click="
                    javaPicker = null;
                    openSettings('java');
                "
            >
                管理 Java
            </button>
            <button
                data-ui="HomeView:6248b7b5543e"
                class="btn btn-ghost"
                :disabled="javaSaving"
                @click="javaPicker = null"
            >
                取消
            </button>
            <button
                data-ui="HomeView:853a319d45b3"
                class="btn btn-gold"
                :disabled="javaSaving"
                @click="saveJavaChoice"
            >
                {{ javaSaving ? '保存中…' : '保存选择' }}
            </button>
        </template>
    </AppModal>
    <!-- 再次启动确认收编 AppModal：内联 style 按钮行 → #actions（.modal-actions）；ESC/焦点托管；
         点遮罩不关闭（与旧行为一致，避免误触绕过警告） -->
    <AppModal
        :open="!!relaunchConfirm"
        title="已有游戏正在运行"
        :dismissable="false"
        @close="relaunchConfirm = null"
    >
        <template v-if="relaunchConfirm">
            <p>
                当前已有一个正在运行的游戏实例。再次启动会开启一个全新实例，
                两个实例同时写同一存档时可能产生冲突。
            </p>
            <p style="color: var(--danger)">
                如果两边进的是同一个世界，请先确认另一个实例已保存退出。
            </p>
        </template>
        <template #actions>
            <template v-if="relaunchConfirm">
                <button class="btn btn-ghost" @click="relaunchConfirm = null">取消</button
                ><button
                    class="btn btn-gold"
                    @click="
                        startVersion(
                            relaunchConfirm.id,
                            relaunchConfirm.createCommandWorld,
                            true
                        );
                        relaunchConfirm = null;
                    "
                >
                    仍要再启动一个
                </button>
            </template>
        </template>
    </AppModal>
    <!-- 强制重启确认收编 AppModal：修复旧弹窗 ESC 无响应（旧按钮文案非「取消」，
         modalFocus 匹配不到 dismiss 按钮；AppModal 的 app:modal-esc 兜底可关） -->
    <AppModal
        :open="!!restartConfirm"
        title="正常退出等待超时"
        :busy="restartBusy"
        :dismissable="false"
        @close="cancelRestartPrompt"
    >
        <template v-if="restartConfirm">
            <p data-ui="HomeView:2925c2d17b9d">
                Minecraft 可能仍在保存世界。建议在游戏内保存退出，然后重试。
            </p>
            <p data-ui="HomeView:37b32c3aeef4" style="color: var(--danger)">
                强制结束可能丢失进度或损坏存档；只有你确认后才会执行。
            </p>
        </template>
        <template #actions>
            <template v-if="restartConfirm">
                <button
                    data-ui="HomeView:1ab5dde2249d"
                    class="btn btn-ghost"
                    :disabled="restartBusy"
                    @click="cancelRestartPrompt"
                >
                    取消重启，继续等待</button
                ><button
                    data-ui="HomeView:df15de437ca0"
                    class="btn btn-danger"
                    :disabled="restartBusy"
                    @click="
                        quickRestart(
                            {
                                id: restartConfirm.id,
                                folder: restartConfirm.folder
                            } as InstalledVersion,
                            restartConfirm.token
                        )
                    "
                >
                    确认强制结束并重启
                </button>
            </template>
        </template>
    </AppModal>
</template>

<style scoped>
.recent-grid,
.instance-grid {
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)) !important;
}
.recent-card {
    min-height: 140px;
    height: auto !important;
}
.hero-content h1 {
    overflow-wrap: break-word;
    word-break: normal;
}
.runtime-strip {
    background: var(--surface-content) !important;
}
.home-side {
    align-self: start;
}

/* Java 选择器外壳已收编 AppModal（宽度档位走全局 .modal--*），旧宽度定制删除 */
.java-picker-description {
    color: var(--text-dim);
    font-size: 12px;
    margin-bottom: 16px;
    overflow-wrap: anywhere;
}
.java-list {
    max-height: 32vh;
    overflow: auto;
    margin-top: 10px;
}
.java-option {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 13px;
    margin-bottom: 8px;
    border: 1px solid var(--border);
    border-radius: 12px;
    cursor: pointer;
}
.java-option:has(input:checked) {
    border-color: var(--accent);
    background: var(--accent-soft);
}
.java-option input {
    accent-color: var(--accent);
    flex: none;
}
.java-option span {
    min-width: 0;
}
.java-option strong,
.java-option small {
    display: block;
}
.java-option small {
    margin-top: 5px;
    font-size: 11px;
    color: var(--text-dim);
    overflow-wrap: anywhere;
}
.home-dashboard {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 286px;
    gap: 14px;
    width: 100%;
    max-width: 1160px;
    margin: 0 auto;
    min-width: 0;
    min-height: calc(100vh - 132px);
}
.home-main {
    display: flex;
    min-width: 0;
    min-height: inherit;
    flex-direction: column;
    gap: 14px;
}
.hero-card {
    position: relative;
    height: var(--banner-h);
    min-height: 348px;
    flex: none;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--border) 95%, white 4%);
    border-radius: 20px;
    background: #17231f;
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.22);
}
.hero-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    transition: opacity var(--motion-carousel) ease;
}
.hero-image.active {
    opacity: 1;
}
.hero-card.no-banner {
    background: var(--surface-content);
    box-shadow: none;
}
.no-banner .hero-content,
.no-banner .hero-content h1,
.no-banner .hero-game-version {
    color: var(--text);
    text-shadow: none;
}
.no-banner .hero-kicker,
.no-banner .hero-settings,
.no-banner .hero-more {
    background: var(--card-2);
    color: var(--text);
    border-color: var(--border);
    backdrop-filter: none;
}
.no-banner .loader-badge {
    color: var(--accent-2);
    background: var(--accent-soft);
}
.hero-shade {
    position: absolute;
    inset: 0;
    background:
        linear-gradient(
            90deg,
            rgba(8, 14, 15, 0.66),
            rgba(8, 14, 15, 0.24) 54%,
            rgba(8, 14, 15, 0.08)
        ),
        linear-gradient(0deg, rgba(5, 10, 9, 0.38), transparent 52%);
}
.hero-content {
    position: relative;
    z-index: 1;
    display: flex;
    height: 100%;
    padding: 48px 44px 32px 44px;
    flex-direction: column;
    align-items: flex-start;
    color: var(--bn-text);
}
/* 像素动画场景文案块：色点 + 眉标 + 标题 + 注（对应上游 copy 列） */
.hero-scene-copy {
    display: flex;
    flex-direction: column;
    gap: 7px;
    margin: 20px 0 0;
    max-width: 560px;
}
.hero-scene-eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: rgba(255, 255, 255, 0.78);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
}
.hero-scene-eyebrow i {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    box-shadow: 0 0 6px rgba(0, 0, 0, 0.25);
}
.hero-scene-title {
    color: #fff;
    font-size: 23px;
    line-height: 1.3;
    font-weight: 700;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.45);
}
.hero-scene-caption {
    color: rgba(255, 255, 255, 0.62);
    font-size: 13px;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
}
.hero-scene-caption em {
    font-style: normal;
    color: rgba(255, 255, 255, 0.45);
}
.hero-scene-copy-enter-active {
    transition:
        opacity 0.5s ease,
        transform 0.5s ease;
}
.hero-scene-copy-enter-from {
    opacity: 0;
    transform: translateY(6px);
}
.hero-scene-copy-leave-active {
    transition: opacity 0.18s ease;
}
.hero-scene-copy-leave-to {
    opacity: 0;
}
.hero-kicker {
    display: inline-flex;
    align-items: center;
    min-height: 34px;
    padding: 0 14px;    border: 1px solid rgba(255, 255, 255, 0.13);
    border-radius: 7px;
    background: rgba(9, 13, 14, 0.48);
    backdrop-filter: blur(12px);
    font-size: 13px;
    font-weight: 650;
}
.hero-content h1 {
    max-width: 100%;
    margin-top: 18px;
    overflow: hidden;
    color: #fff;
    font-size: clamp(46px, 5.2vw, 64px);
    font-weight: 850;
    line-height: 1.15;
    letter-spacing: -1px;
    text-overflow: ellipsis;
    text-shadow: 0 4px 24px rgba(0, 0, 0, 0.32);
    white-space: nowrap;
}
.hero-content h1.long-name {
    font-size: clamp(28px, 3.2vw, 42px);
    letter-spacing: -0.5px;
}
.hero-metadata-slot {
    width: 100%;
    min-height: 130px;
    position: relative;
}
.hero-metadata {
    width: 100%;
}
.instance-switch-enter-active,
.instance-switch-leave-active {
    transition:
        opacity 100ms ease,
        transform 100ms ease;
}
.instance-switch-enter-from {
    opacity: 0;
    transform: translateY(2px);
}
.instance-switch-leave-to {
    opacity: 0;
    transform: translateY(-2px);
}
@media (prefers-reduced-motion: reduce) {
    .instance-switch-enter-active,
    .instance-switch-leave-active {
        transition: none;
    }
}
.hero-edition {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    margin-top: 16px;
    font-size: 17px;
    font-weight: 650;
    overflow-wrap: anywhere;
}
.hero-game-version {
    color: #fff;
}
.loader-badge {
    padding: 5px 11px;
    border: 1px solid color-mix(in srgb, var(--accent-2) 34%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 30%, rgba(20, 30, 24, 0.46));
    color: #f6fff8;
    font-size: 11px;
    font-weight: 650;
}
.hero-actions {
    display: flex;
    width: 100%;
    margin-top: auto;
    align-items: flex-end;
    justify-content: space-between;
    gap: 18px;
}
.hero-secondary-actions,
.launch-combo {
    display: flex;
    align-items: stretch;
}
.hero-settings,
.hero-more {
    height: 52px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(10, 16, 17, 0.58);
    color: #f4f7f5;
    backdrop-filter: blur(13px);
    cursor: pointer;
}
.hero-settings {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-width: 150px;
    padding: 0 18px;
    border-radius: 10px 0 0 10px;
    font-family: inherit;
    font-size: 14px;
    font-weight: 650;
}
.hero-settings svg {
    width: 18px;
    height: 18px;
}
.hero-more {
    width: 52px;
    border-left: 0;
    border-radius: 0 10px 10px 0;
}
.hero-more svg {
    width: 19px;
    height: 19px;
}
.hero-settings:hover:not(:disabled),
.hero-more:hover:not(:disabled) {
    background: rgba(19, 29, 29, 0.76);
}
.hero-settings:disabled,
.hero-more:disabled {
    opacity: 0.45;
    cursor: default;
}
.launch-combo {
    min-width: 310px;
    height: 76px;
    border-radius: 14px;
    overflow: hidden;
    box-shadow:
        0 12px 32px color-mix(in srgb, var(--accent) 38%, transparent),
        0 2px 0 color-mix(in srgb, white 14%, transparent) inset;
    transition:
        transform var(--motion-normal) var(--ease-out),
        box-shadow 0.22s ease;
}
.launch-combo:hover {
    transform: translateY(-2px);
    box-shadow:
        0 16px 40px color-mix(in srgb, var(--accent) 46%, transparent),
        0 2px 0 color-mix(in srgb, white 16%, transparent) inset;
}
.launch-combo:active {
    transform: translateY(0) scale(0.99);
}
.launch-main,
.launch-arrow {
    position: relative;
    overflow: hidden;
    border: 0;
    background: var(--accent-grad);
    color: var(--on-accent);
    cursor: pointer;
}
.launch-main {
    flex: 1;
    min-width: 0;
    padding: 0 26px;
    font-family: inherit;
    font-size: 21px;
    font-weight: 800;
    letter-spacing: 0.5px;
}
.launch-content {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
}
.launch-content svg {
    width: 22px;
    height: 22px;
    flex: none;
}
.launch-content span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.launch-progress {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(255, 255, 255, 0.25);
    transition: width 0.25s ease;
}
.launch-main:disabled {
    cursor: not-allowed;
    filter: saturate(0.75);
}
.launch-arrow {
    width: 62px;
    border-left: 1px solid rgba(255, 255, 255, 0.22);
}
.launch-arrow:hover,
.launch-main:hover:not(:disabled) {
    filter: brightness(1.08);
}
.launch-arrow svg {
    width: 22px;
    height: 22px;
    transition: transform 0.18s ease;
}
.launch-arrow svg.open {
    transform: rotate(180deg);
}

.runtime-strip {
    position: relative;
    display: grid;
    grid-template-columns: 1.12fr 0.95fr 0.92fr;
    min-height: 82px;
    flex: none;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 15px;
    background: color-mix(in srgb, var(--card) 80%, transparent);
    box-shadow: var(--shadow);
    backdrop-filter: blur(18px) saturate(130%);
    -webkit-backdrop-filter: blur(18px) saturate(130%);
}
/* 悬浮浮块：跟随指针在三格间平滑滑动（浮起+落下+格间转移过渡） */
.runtime-blob {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 0;
    border-radius: 12px;
    margin: var(--space-1) 0;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    opacity: 0;
    transform: scale(0.97);
    transition:
        left var(--motion-normal) var(--ease-out),
        width var(--motion-normal) var(--ease-out),
        opacity 0.18s ease,
        transform 0.2s ease;
    pointer-events: none;
}
.runtime-blob.on {
    opacity: 1;
    transform: scale(1);
}
.runtime-item {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) 15px;
    align-items: center;
    gap: 11px;
    min-width: 0;
    padding: 0 18px;
    border: 0;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    transition: transform 0.18s cubic-bezier(0.22, 0.9, 0.32, 1.15);
}
.runtime-item + .runtime-item {
    border-left: 1px solid var(--border);
}
.runtime-item:hover {
    color: var(--accent-2);
}
.runtime-item > svg:first-child {
    width: 27px;
    height: 27px;
    color: var(--text);
}
.runtime-item > span {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 4px;
}
.runtime-item small {
    color: var(--text-dim);
    font-size: 12px;
}
.runtime-item strong {
    overflow: hidden;
    font-size: 13px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.runtime-chevron {
    width: 14px;
    height: 14px;
    color: var(--text-dim);
    opacity: 0.7;
}
.runtime-state > svg:first-child {
    color: var(--accent-2);
}
.runtime-state strong {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--accent-2);
}
.runtime-state i {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: #9ca3af;
}
.runtime-state.ready i,
.runtime-state.running i {
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
}
.runtime-state.error strong {
    color: var(--danger);
}
.runtime-state.error i {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
}

.instances-block {
    min-width: 0;
    margin-top: 14px;
}
.instances-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 13px;
}
.instances-head h2 {
    font-size: 17px;
    font-weight: 750;
    display: flex;
    align-items: center;
    gap: 9px;
}
/* 区块标题前的主题色短竖线：视觉锚点 */
.instances-head h2::before {
    content: '';
    width: 4px;
    height: 17px;
    border-radius: 2px;
    background: var(--accent-grad);
    flex: none;
}
.manage-instances {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 34px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--card-2);
    color: var(--text-dim);
    font-family: inherit;
    font-size: 12px;
    font-weight: 550;
    cursor: pointer;
}
.manage-instances:hover {
    color: var(--text);
    border-color: var(--border-strong);
}
.manage-instances svg {
    width: 15px;
    height: 15px;
}
.instance-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 14px;
}
/* Cards appear together so every action is available at the same time. */
.instance-card {
    position: relative;
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr);
    grid-template-rows: 1fr auto;
    gap: 8px 8px;
    min-width: 0;
    height: 132px;
    min-height: 132px;
    padding: 17px 14px 13px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: color-mix(in srgb, var(--card) 82%, transparent);
    cursor: pointer;
    /* hover 只动底色/边框（L 行 hover 规则：不做位移）；入场动画保留 */
    transition:
        border-color 0.18s ease,
        background 0.18s ease;
    animation: card-in var(--motion-enter) var(--ease-out) backwards;
}
@keyframes card-in {
    from {
        opacity: 0;
    }
    to {
        opacity: 1;
    }
}
/* 卡片 hover 对齐统一规则：背景 + 边框提亮，不做位移、不加阴影（页内卡片无阴影，UI_SYSTEM §2）；
 * 主操作（play）的 accent 点亮只发生在指向按钮本身 :hover（.instance-play:hover 既有规则），行 hover 不点亮。 */
.instance-card:hover {
    border-color: var(--border-strong);
    background: var(--card-2);
}
.instance-card.selected {
    border-color: var(--accent-2);
    box-shadow: inset 0 0 0 1px var(--accent);
}
.instance-icon {
    align-self: center;
    width: 36px;
    height: 36px;
}
.instance-icon.image {
    object-fit: contain;
    image-rendering: pixelated;
}
.instance-copy {
    grid-column: 2;
    padding-right: 7px;
    align-self: center;
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 4px;
}
.instance-copy strong {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: 13px;
    line-height: 1.4;
    font-weight: 650;
    overflow-wrap: anywhere;
    word-break: break-all;
}
.instance-copy span,
.instance-last {
    overflow: hidden;
    color: var(--text-dim);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.instance-more {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 20px;
    height: 20px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
}
.instance-more:hover {
    background: var(--hover);
    color: var(--text);
}
.instance-more svg {
    width: 15px;
    height: 15px;
}
.instance-last {
    grid-column: 1 / -1;
    grid-row: 2;
    padding-right: 42px;
    align-self: center;
}
.instance-play {
    grid-column: 2;
    grid-row: 2;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    height: 32px;
    justify-self: end;
    border: 0;
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 18%, var(--card-2));
    color: var(--accent-2);
    cursor: pointer;
}
.instance-play:hover:not(:disabled) {
    background: var(--accent);
    color: var(--on-accent);
}
.instance-play:disabled {
    opacity: 0.45;
    cursor: default;
}
.instance-play svg {
    width: 15px;
    height: 15px;
}
/* 实例空态已收编 EmptyState，旧虚线按钮样式删除 */

.home-side {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 12px;
}
.home-creator {
    margin-top: 0;
    border-color: color-mix(in srgb, var(--accent) 26%, var(--border));
    box-shadow:
        inset 0 0 0 1px color-mix(in srgb, var(--accent) 10%, transparent),
        var(--shadow);
}
.account-panel,
.skin-panel {
    border: 1px solid var(--border);
    border-radius: 15px;
    background: var(--surface-content);
    box-shadow: var(--shadow);
}
.account-panel {
    min-height: 146px;
    padding: 18px;
}
.account-head {
    display: flex;
    align-items: center;
    gap: 13px;
}
.account-head :deep(.mc-avatar) {
    border-radius: 12px;
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--text) 8%, transparent);
}
.account-copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 5px;
}
.account-copy strong {
    overflow: hidden;
    font-size: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.account-copy span {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--accent-2);
    font-size: 11px;
}
.account-copy span i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
}
.account-copy .offline-state {
    color: var(--text-dim);
}
.account-more,
.skin-refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
}
.account-more:hover,
.skin-refresh:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
}
.account-more svg,
.skin-refresh svg {
    width: 17px;
    height: 17px;
}
.account-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 54px;
    height: 54px;
    border: 1px dashed var(--border-strong);
    border-radius: 12px;
    color: var(--text-dim);
    font-size: 20px;
}
.account-provider {
    display: grid;
    grid-template-columns: 26px minmax(0, 1fr) 15px;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    margin-top: 15px;
    padding: 0 11px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--card-2);
    color: var(--text);
    font-family: inherit;
    font-size: 12px;
    font-weight: 550;
    text-align: left;
    cursor: pointer;
}
.provider-mark.microsoft {
    background: transparent;
    border-radius: 0;
}
.provider-mark.microsoft svg {
    width: 22px;
    height: 22px;
}
.account-provider:hover {
    border-color: var(--border-strong);
}
.account-provider > svg {
    width: 15px;
    height: 15px;
    color: var(--text-dim);
}
.provider-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: linear-gradient(135deg, #f35325 0 48%, #81bc06 48% 100%);
    color: #fff;
    font-size: 10px;
    font-weight: 800;
}
.provider-mark.yggdrasil {
    background: linear-gradient(135deg, #65bd78, #268e54);
}
.provider-mark .provider-logo {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    object-fit: cover;
    image-rendering: auto;
    background: #fff;
}
.provider-mark.offline {
    background: var(--card);
    color: var(--text-dim);
}

.skin-panel {
    min-height: 400px;
    padding: 16px 15px 12px;
}
.skin-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 0 2px 8px;
}
.skin-head > div {
    display: flex;
    flex-direction: column;
    gap: 3px;
}
.skin-head h3 {
    font-size: 14px;
    font-weight: 700;
}
.skin-head span {
    color: var(--text-dim);
    font-size: 10px;
}
.skin-refresh:disabled {
    opacity: 0.4;
    cursor: default;
}
.skin-refresh .spinning {
    animation: spin 0.8s linear infinite;
}
.skin-stage {
    position: relative;
    height: 300px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--border) 78%, transparent);
    border-radius: 12px;
    background:
        radial-gradient(
            circle at 50% 82%,
            color-mix(in srgb, var(--accent) 13%, transparent),
            transparent 38%
        ),
        linear-gradient(180deg, transparent, color-mix(in srgb, var(--bg) 18%, transparent));
}
.skin-stage :deep(.viewer3d) {
    height: 100%;
    --sv3d-height: 100%;
    background: transparent;
}

.skin-overlay {
    position: absolute;
    z-index: 2;
    right: 12px;
    bottom: 12px;
    left: 12px;
    display: flex;
    min-height: 34px;
    align-items: center;
    justify-content: center;
    gap: 9px;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: color-mix(in srgb, var(--card) 78%, transparent);
    color: var(--text-dim);
    font-family: inherit;
    font-size: 11px;
    font-weight: 550;
    backdrop-filter: blur(12px);
}
.skin-overlay.action {
    cursor: pointer;
}
.skin-overlay.action:hover {
    color: var(--text);
}
.skin-overlay.error {
    color: var(--danger);
}
.skin-tip {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    width: 100%;
    margin-top: 8px;
    border: 0;
    background: transparent;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 10px;
    font-weight: 500;
    cursor: pointer;
}
.skin-tip:hover {
    color: var(--accent-2);
}
.skin-tip svg {
    width: 12px;
    height: 12px;
}

.menu-overlay {
    position: fixed;
    inset: 0;
        z-index: var(--z-menu);
}
.float-menu {
    position: fixed;
        z-index: var(--z-menu);
    max-height: 280px;
    overflow-y: auto;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: color-mix(in srgb, var(--card) 94%, transparent);
    box-shadow: var(--shadow-lg);
    backdrop-filter: blur(24px);
}
/* 实例卡 ⋯ 菜单已收编 AppMenu（定位/分组/键盘由组件托管），旧 .card-float-menu 定制样式删除 */
.menu-item:disabled {
    opacity: 0.45;
    cursor: not-allowed;
}
.menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 12px;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
}
.menu-item:hover,
.menu-item.active {
    background: var(--accent-soft);
    color: var(--accent-2);
}
.menu-item.danger {
    color: var(--danger);
}
.menu-spacer {
    width: 12px;
    flex: none;
}
.menu-empty {
    padding: 20px 8px;
    color: var(--text-dim);
    font-size: 12px;
    text-align: center;
}

/* 日志弹窗外壳已收编 AppModal（遮罩/标题/按钮行走全局 .modal 体系），旧 .log-mask/.log-dialog 样式删除 */
.log-subtitle {
    margin: -6px 0 0;
    color: var(--text-dim);
    font-size: 11px;
}
.log-failure {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 18px;
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 12px;
}
.log-failure button {
    padding: 6px 10px;
    border: 1px solid var(--danger-border);
    border-radius: 7px;
    background: transparent;
    color: var(--danger);
    cursor: pointer;
}
.log-body {
    min-height: 220px;
    /* AppModal 外壳下给日志区内部滚动上限，按钮行不随日志滚走 */
    max-height: min(460px, 58vh);
    overflow: auto;
    padding: 14px 18px;
    background: color-mix(in srgb, var(--bg) 42%, transparent);
    user-select: text;
}
.log-body pre {
    display: flex;
    flex-direction: column;
    color: var(--text-dim);
    font:
        11.5px/1.65 'Cascadia Code',
        Consolas,
        monospace;
    white-space: pre-wrap;
    word-break: break-all;
}
.log-empty {
    padding: 70px 0;
    color: var(--text-dim);
    text-align: center;
}

@media (max-width: 1180px) {
    .home-dashboard {
        grid-template-columns: minmax(0, 1fr) 248px;
        gap: 12px;
    }
    .hero-content {
        padding: 38px 26px 25px;
    }
    .launch-combo {
        min-width: 242px;
        height: 62px;
    }
    .launch-main {
        padding: 0 16px;
        font-size: 17px;
    }
    .launch-arrow {
        width: 50px;
    }
    .hero-settings {
        min-width: 126px;
        padding: 0 13px;
    }
    .hero-more {
        width: 46px;
    }
    .runtime-item {
        grid-template-columns: 28px minmax(0, 1fr);
        padding: 0 12px;
    }
    .runtime-item > svg:first-child {
        width: 23px;
        height: 23px;
    }
    .runtime-chevron {
        display: none;
    }
    .instance-grid {
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }
}

@media (max-width: 1010px) {
    .home-dashboard {
        grid-template-columns: minmax(0, 1fr) 226px;
    }
    .hero-card {
        min-height: 320px;
    }
    .hero-content h1 {
        font-size: 42px;
    }
    .hero-actions {
        gap: 10px;
    }
    .hero-settings {
        min-width: 112px;
        font-size: 12px;
    }
    .launch-combo {
        min-width: 210px;
    }
    .launch-main {
        font-size: 15px;
    }
    .runtime-item {
        padding: 0 9px;
        gap: 8px;
    }
    .runtime-item strong {
        font-size: 11px;
    }
    .skin-panel {
        padding-inline: 10px;
    }
}

@media (max-width: 1080px) {
    .hero-actions {
        flex-wrap: wrap;
        align-items: stretch;
        gap: 12px;
    }
    .hero-secondary-actions {
        width: 100%;
    }
    .hero-settings {
        height: 38px;
        flex: 1;
    }
    .hero-more {
        height: 38px;
    }
    .launch-combo {
        width: 100%;
        min-width: 0;
        height: 54px;
    }
    .hero-content {
        padding: 24px;
    }
    .hero-content h1,
    .hero-content h1.long-name {
        font-size: clamp(25px, 3.2vw, 36px);
    }
    .hero-metadata-slot {
        min-height: 110px;
    }
    .hero-edition {
        font-size: 14px;
        margin-top: 10px;
    }
}

@media (max-height: 760px) {
    .home-dashboard {
        min-height: 660px;
    }
    .hero-card {
        height: 340px;
    }
    .hero-content {
        padding-top: 30px;
    }
    .skin-stage {
        height: 230px;
    }
    .skin-panel {
        min-height: 322px;
    }
}

.home-dashboard {
    grid-template-columns: minmax(0, 1fr) minmax(250px, 290px);
    gap: 16px;
}
.hero-card {
    height: var(--banner-h);
    max-height: 360px;
    min-height: 320px;
}
.hero-content {
    padding: 24px;
    justify-content: space-between;
}
.hero-metadata-slot {
    min-height: 0;
}
.hero-kicker {
    font-size: 12px;
    align-self: flex-start;
    padding: 5px 10px;
}
.hero-content h1,
.hero-content h1.long-name {
    font-size: clamp(24px, 2.6vw, 32px);
    line-height: 1.25;
    max-width: 100%;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
}
.hero-edition {
    margin-top: 8px;
    gap: 10px;
}
.hero-game-version {
    font-size: 15px;
}
.launch-main {
    min-height: 56px;
    min-width: 180px;
    font-size: 21px;
    padding: 0 24px;
}
.launch-arrow {
    width: 48px;
}
.hero-actions {
    gap: 12px;
    flex-wrap: wrap;
}
.runtime-strip {
    min-height: 68px;
    padding: 8px;
}
.runtime-item {
    padding: 10px 12px;
}
.instance-grid {
    display: flex !important;
    flex-direction: column;
    gap: 0;
}
.home-dashboard .instance-card {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr) auto 36px 40px;
    gap: 12px;
    align-items: center;
    min-height: 72px;
    height: auto;
    padding: 12px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    box-shadow: none;
    background: transparent;
}
.instance-card.selected {
    background: var(--accent-soft);
}
.instance-card:hover {
    background: var(--hover);
}
.instance-card .instance-icon {
    width: 36px;
    height: 36px;
}
.instance-card .instance-copy {
    min-width: 0;
}
.instance-copy strong {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: normal;
}
.instance-copy span {
    white-space: nowrap;
    text-overflow: ellipsis;
    overflow: hidden;
}
.instance-more,
.instance-last,
.instance-play {
    position: static;
    margin: 0;
    grid-row: 1;
}
.instance-last {
    grid-column: 3;
    font-size: 12px;
    white-space: nowrap;
}
.instance-more {
    grid-column: 4;
}
.instance-play {
    grid-column: 5;
    width: 40px;
    height: 34px;
}
.instances-block {
    background: var(--surface-content);
    border-radius: var(--radius-lg);
    padding: 16px;
}
.instances-head {
    margin-bottom: 8px;
}
.account-panel {
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
    border-bottom: 0;
    box-shadow: none;
    padding: 16px;
}
.skin-panel {
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
    margin-top: -16px;
    border-top: 0;
    box-shadow: none;
    padding: 16px;
}
.skin-stage {
    height: 260px;
    min-height: 220px;
}
.account-head {
    gap: 10px;
}
.home-side {
    gap: 16px;
}
.home-creator {
    box-shadow: none;
    margin-top: 0;
}
.hero-shade {
    background: linear-gradient(
        180deg,
        rgba(0, 0, 0, 0.16),
        rgba(0, 0, 0, 0.18) 30%,
        rgba(0, 0, 0, 0.68)
    );
}
@media (max-width: 1100px) {
    .home-dashboard {
        grid-template-columns: minmax(0, 1fr);
    }
    .home-side {
        display: grid;
        grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
        align-items: start;
    }
    .account-panel {
        border-radius: var(--radius-lg);
    }
    .skin-panel {
        margin-top: 0;
        border-radius: var(--radius-lg);
        grid-column: 2;
        grid-row: 1/3;
    }
    .home-creator {
        grid-column: 1;
    }
    .hero-card {
        max-height: 360px;
    }
}
@media (max-width: 700px) {
    .home-side {
        grid-template-columns: minmax(0, 1fr);
    }
    .skin-panel,
    .home-creator {
        grid-column: 1;
        grid-row: auto;
    }
    .home-dashboard .instance-card {
        grid-template-columns: 28px minmax(0, 1fr) 32px 36px;
        gap: 8px;
    }
    .instance-last {
        grid-row: 2;
        grid-column: 2;
        font-size: 12px;
    }
    .instance-more {
        grid-column: 3;
    }
    .instance-play {
        grid-column: 4;
    }
    .hero-content {
        padding: 16px;
    }
    .launch-main {
        min-width: 140px;
    }
    .runtime-strip {
        flex-wrap: wrap;
    }
    .runtime-item {
        min-width: 140px;
    }
    .hero-card {
        min-height: 330px;
    }
}

.home-side {
    align-self: start;
}
.skin-panel {
    flex: none;
    min-height: 0;
}
.home-side .skin-stage {
    height: 230px;
    min-height: 0;
}
.skin-panel .skin-tip {
    margin-bottom: 0;
}
.account-panel {
    padding: 16px;
}
.hero-card .hero-image {
    object-position: center 42%;
}

/* ---------------- Android 主页（按手绘布景落地） ----------------
   中：舞台（背景图 + 框起的版本名 + 两角操作 + 状态 pill）；
   右：操作坞（账号卡 → 皮肤预览(3D) → 启动按钮）。固定一屏不滚动。 */
.m-launchpad {
    position: absolute;
    /* shell 的 padding 已消化系统栏安全区，这里只需要常规内边距 */
    inset: 8px;
    display: flex;
    gap: 10px;
    min-height: 0;
    overflow: hidden;
}
/* ---------- 中：舞台 ---------- */
.m-stage {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--border) 95%, white 4%);
    border-radius: var(--radius-lg);
    background: var(--surface-content);
    box-shadow: var(--shadow);
}
.m-stage-bg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 42%;
}
.m-stage-shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(200deg, rgba(8, 12, 13, 0.42) 0%, rgba(8, 12, 13, 0.2) 45%, rgba(8, 12, 13, 0.66) 100%);
}
.m-stage-head {
    position: absolute;
    top: 12px;
    left: 14px;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 10px;
}
.m-kicker {
    padding: 4px 10px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 7px;
    background: rgba(9, 13, 14, 0.5);
    color: #f4f7f5;
    font-size: var(--text-xs);
    font-weight: 650;
    backdrop-filter: blur(10px);
}
/* 版本名称：框起的胶囊元素（用户草稿要求） */
.m-namechip {
    max-width: 46vw;
    height: 34px;
    padding: 0 14px;
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 999px;
    background: rgba(9, 13, 14, 0.56);
    color: #fff;
    font-family: inherit;
    font-size: var(--text-md);
    font-weight: 750;
    text-overflow: ellipsis;
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
    backdrop-filter: blur(10px);
    transition: border-color 0.15s ease;
}
.m-namechip:hover {
    border-color: rgba(255, 255, 255, 0.55);
}
.m-stage.no-version .m-namechip {
    border-style: dashed;
    color: rgba(244, 247, 245, 0.85);
}
/* 右上：运行状态 pill */
.m-status {
    position: absolute;
    top: 15px;
    right: 14px;
    z-index: 1;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 56%;
    height: 30px;
    padding: 0 12px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 999px;
    background: rgba(9, 13, 14, 0.5);
    color: rgba(244, 247, 245, 0.88);
    font-size: var(--text-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    backdrop-filter: blur(10px);
}
.m-status i {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-dim);
}
.m-status.is-running i {
    background: var(--accent-2);
}
.m-status.is-ready i {
    background: var(--ok);
}
.m-status.is-error i {
    background: var(--danger);
}
/* 底部两角：版本设置 / 更改版本 */
.m-stage-corner {
    position: absolute;
    bottom: 14px;
    z-index: 1;
}
.m-stage-corner.left {
    left: 14px;
}
.m-stage-corner.right {
    right: 14px;
}
.m-corner-btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 40px;
    padding: 0 16px;
    border: 1px solid rgba(255, 255, 255, 0.22);
    border-radius: 999px;
    background: rgba(9, 13, 14, 0.56);
    color: #f4f7f5;
    font-family: inherit;
    font-size: var(--text-sm);
    font-weight: 650;
    cursor: pointer;
    backdrop-filter: blur(12px);
    transition:
        background 0.15s ease,
        border-color 0.15s ease;
}
.m-corner-btn:hover {
    background: rgba(20, 30, 32, 0.7);
    border-color: rgba(255, 255, 255, 0.4);
}
.m-corner-btn svg {
    width: 15px;
    height: 15px;
    opacity: 0.85;
}
/* ---------- 右：操作坞 ---------- */
.m-dock {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: none;
    width: 180px;
    min-height: 0;
}
.m-account {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
    height: 58px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--card);
    color: var(--text);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.15s ease;
}
.m-account:hover {
    border-color: var(--border-strong);
}
.m-account-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
}
.m-account-info b {
    overflow: hidden;
    font-size: var(--text-md);
    font-weight: 750;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.m-account-info i {
    overflow: hidden;
    color: var(--text-dim);
    font-size: var(--text-xs);
    font-style: normal;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.m-skin {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--card);
    cursor: pointer;
    overflow: hidden;
}
.m-skin-stage {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--bg) 42%, transparent);
}
.m-skin-stage :deep(.viewer3d) {
    width: 100%;
    height: 100%;
}
.m-skin-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--bg) 40%, transparent);
}
.m-skin-overlay .spin {
    width: 22px;
    height: 22px;
    border: 2px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: mspin 0.8s linear infinite;
}
@keyframes mspin {
    to {
        transform: rotate(360deg);
    }
}
.m-skin-hint {
    flex: none;
    padding: 7px 12px;
    color: var(--text-dim);
    font-size: var(--text-xs);
    text-align: center;
    border-top: 1px solid var(--border);
}
/* 启动按钮 */
.m-launch {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    flex: none;
    width: 100%;
    height: 54px;
    border: none;
    border-radius: var(--radius-lg);
    background: var(--accent-grad);
    color: var(--on-accent);
    font-family: inherit;
    font-size: 17px;
    font-weight: 800;
    letter-spacing: 0.5px;
    cursor: pointer;
    overflow: hidden;
    box-shadow: 0 10px 26px color-mix(in srgb, var(--accent) 36%, transparent);
}
.m-launch svg {
    width: 20px;
    height: 20px;
    flex: none;
}
.m-launch:disabled {
    filter: saturate(0.7);
    cursor: not-allowed;
}
</style>