import type { InjectionKey } from 'vue';
import SupplementalModsResult from '../../components/SupplementalModsResult.vue';
import { shouldReportGameCrash, signedExitCode } from '@shared/gameExit';
import LaunchNotice from '../../components/LaunchNotice.vue';
import { pollTickets } from '../../voxlinkTickets';
import CreatorMotto from '../../components/CreatorMotto.vue';
import ModpackSupplement from '../../components/ModpackSupplement.vue';
import { instanceCenter, openInstanceCenter } from '../../instanceCenter';
import { loadExitNotices, clearNotices } from '../state';
import { useNavigationBubble } from '../../composables/useNavigationBubble';
import { usePlatformUpdate } from '../../composables/usePlatformUpdate';
import {
    computed,
    defineAsyncComponent,
    nextTick,
    onMounted,
    onUnmounted,
    provide,
    reactive,
    ref,
    watch
} from 'vue';
import { MASCOT_INTERACTIVE } from '../../mascotInteraction';
import type { Component } from 'vue';
import { backgroundImageEffect } from '@shared/appearancePolicy';
import { taskProgressPercent } from '@shared/taskProgress';
import {
    applyUpdate,
    cancelTask,
    errText,
    exportLaunchLogs,
    formatSpeed,
    getConfigStatus,
    getSettings,
    installModpack,
    onGameDirDone,
    onInstallDone,
    onLaunchLog,
    onLaunchState,
    onProgress,
    onTaskDone,
    onUpdatePrompt,
    onUpdateReady,
    onUpdateSlowHint,
    pauseTask,
    probeModpack,
    probeImport,
    resetSettingsToDefaults,
    resumeTask,
    selectFile,
    skipUpdateVersion,
    startUpdateDownload,
    filePathFromDrop,
    platform,
    sendIpc,
    subscribeIpc
} from '../../core/ipc';
import type { ReleaseInfo } from '@shared/types';
import { QQ_GROUP_NUMBER } from '@shared/branding';
import UpdateModal from '../../components/UpdateModal.vue';
import {
    applyLaunchState,
    dismissTask,
    enterEditMode,
    exitEditMode,
    finalizeTask,
    markNoticesRead,
    recordLastPlayed,
    refreshAccounts,
    refreshInstalled,
    resetProgressMono,
    stageLabel,
    store,
    toast,
    upsertTaskProgress
} from '../state';
import type { ViewName } from '../state';
import type {
    ModpackInfo,
    ThemeName,
    WorldImportInfo,
    YggdrasilProviderInput
} from '@shared/types';
import { managedImageUrl } from '../../managedAssets';
import Toasts from '../../components/Toasts.vue';
import { installVisualDesign, appearancePreview, designStageReady } from '../../visualDesign';
import EditPanel from '../../components/EditPanel.vue';
import { waitForBootTasks, sealBootTasks } from '../../core/runtime';
import { installAppearanceRuntime } from '../../core/ui';
import { acceptsImportDrag, showsImportOverlay } from '@shared/dropIntent';
import { updateNotes, latestUpdateNote } from '@shared/updateNotes';
import HomeView from '../../views/HomeView.vue';
import brandHead from '../../assets/splash-face.png';
import ModDropModal from '../../components/ModDropModal.vue';
import WorldImportModal from '../../components/WorldImportModal.vue';

export function useAppShell() {
const isMac = platform() === 'darwin';

const { updateReadyMessage } = usePlatformUpdate();

// 非首屏视图全部懒加载：首屏只打包/挂载 HomeView，其余视图拆独立 chunk 按需拉取
// （渲染层常驻内存大头之一；配合 memTrim/idleTrim 的静默瘦身）。
const InstanceCenter = defineAsyncComponent(() => import('../../components/InstanceCenter.vue'));

const GameView = defineAsyncComponent(() => import('../../views/GameView.vue'));

const ModsView = defineAsyncComponent(() => import('../../views/ModsView.vue'));

const RecordingsView = defineAsyncComponent(() => import('../../views/RecordingsView.vue'));

const ProjectionsView = defineAsyncComponent(() => import('../../views/ProjectionsView.vue'));

const MascotStage = defineAsyncComponent(() => import('../../components/MascotStage.vue'));

const mascotOpen = ref(false);

const mascotReady = ref(false);

const mascotBusy = ref(false);

const mascotSoftware = ref(false);

// Give the active logo interaction priority over decorative skin previews.
provide(
    MASCOT_INTERACTIVE,
    computed(() => mascotOpen.value && (!mascotReady.value || mascotBusy.value))
);

const mascotRef = ref<{ flush: () => Promise<void> }>();

const mascotKeyboard = ref(false);

function openMascots(event: MouseEvent) {
    if (mascotOpen.value) return;
    mascotKeyboard.value = event.detail === 0;
    mascotReady.value = false;
    mascotSoftware.value = false;
    mascotOpen.value = true;
}

function closeMascots() {
    mascotOpen.value = false;
    mascotBusy.value = false;
    void nextTick(() => document.querySelector<HTMLButtonElement>('.brand-avatar')?.focus());
}

const PacksView = defineAsyncComponent(() => import('../../views/PacksView.vue'));

const ShadersView = defineAsyncComponent(() => import('../../views/ShadersView.vue'));

const KeysView = defineAsyncComponent(() => import('../../views/KeysView.vue'));

const BridgeView = defineAsyncComponent(() => import('../../views/BridgeView.vue'));

const SkinsView = defineAsyncComponent(() => import('../../views/SkinsView.vue'));

const CommunityView = defineAsyncComponent(() => import('../../views/CommunityView.vue'));

const ServersView = defineAsyncComponent(() => import('../../views/ServersView.vue'));

const FriendConnectView = defineAsyncComponent(() => import('../../views/FriendConnectView.vue'));

const SettingsView = defineAsyncComponent(() => import('../../views/SettingsView.vue'));

const AccountsView = defineAsyncComponent(() => import('../../views/AccountsView.vue'));

// Vite 的全局 define 在 script 中解析；模板直接访问会被 Vue 当作组件实例字段。
const appVersion = __APP_VERSION__;

const viewMap: Record<ViewName, Component> = {
    home: HomeView,
    game: GameView,
    mods: ModsView,
    packs: PacksView,
    recordings: RecordingsView,
    projections: ProjectionsView,
    shaders: ShadersView,
    keys: KeysView,
    bridge: BridgeView,
    skins: SkinsView,
    community: CommunityView,
    servers: ServersView,
    friends: FriendConnectView,
    settings: SettingsView,
    accounts: AccountsView
};

const currentComponent = computed(() => viewMap[store.currentView]);

const navItems: Array<{ key: ViewName; label: string; icon: string }> = [
    {
        key: 'home',
        label: '首页',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/><path d="M9 21v-6h6v6"/></svg>'
    },
    {
        key: 'game',
        label: '游戏版本',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="7" width="20" height="11" rx="5.5"/><path d="M7.5 10.8v3.4M5.8 12.5h3.4"/><circle cx="15.6" cy="11.9" r="0.6" fill="currentColor" stroke="none"/><circle cx="18" cy="13.6" r="0.6" fill="currentColor" stroke="none"/></svg>'
    },
    {
        key: 'friends',
        label: '联机',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="8" cy="8" r="3"/><path d="M2 21v-3a6 6 0 0 1 12 0v3M16 5a3 3 0 0 1 0 6M18 15a5 5 0 0 1 4 5"/></svg>'
    },
    {
        key: 'keys',
        label: '默认配置',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>'
    },
    {
        key: 'skins',
        label: '皮肤',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 4-6 3 2 5 3-1v9h8v-9l3 1 2-5-6-3a3 3 0 0 1-6 0Z"/></svg>'
    },
    {
        key: 'community',
        label: '社区资源',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a13.5 13.5 0 0 1 0 18"/><path d="M12 3a13.5 13.5 0 0 0 0 18"/></svg>'
    },
    {
        key: 'settings',
        label: '设置',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06-.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06-.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z"/></svg>'
    }
];

/** 功能禁用判定 */
const isFeatureOff = (key: string): boolean =>
    (store.settings?.disabledFeatures ?? []).includes(key);

/** 过滤禁用功能后的主导航 */
const visibleNavItems = computed(() => navItems.filter((n) => !isFeatureOff(n.key)));

/** 过滤禁用功能后的资源管理子项 */
const visibleResourceSubItems = computed(() =>
    resourceSubItems.filter((s) => !isFeatureOff(s.key))
);

/** 资源管理子级菜单（模组/资源包/光影包），按游戏版本管理对应目录 */
const resourceSubItems: Array<{ key: ViewName; label: string; icon: string }> = [
    {
        key: 'mods',
        label: '模组',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8 12 3 3 8v8l9 5 9-5Z"/><path d="m3 8 9 5 9-5"/><path d="M12 13v8"/></svg>'
    },
    {
        key: 'packs',
        label: '资源包',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/></svg>'
    },
    {
        key: 'shaders',
        label: '光影包',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>'
    },

    {
        key: 'recordings',
        label: '录像',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="14" height="14" rx="3"/><path d="m17 10 4-3v10l-4-3"/></svg>'
    },
    {
        key: 'projections',
        label: '投影',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m12 3 9 5v8l-9 5-9-5V8zM3 8l9 5 9-5M12 13v8"/></svg>'
    },
    {
        key: 'bridge',
        label: 'MOD 面板',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/><circle cx="12" cy="12" r="3.5"/></svg>'
    },
    {
        key: 'servers',
        label: '服务器',
        icon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>'
    }
];

/** 资源管理组是否展开（默认折叠；当前在其中任一子页时强制展开高亮） */
const resourceExpanded = ref(localStorage.getItem('kamucl.resourceExpanded') === 'true');

watch(resourceExpanded, (value) => localStorage.setItem('kamucl.resourceExpanded', String(value)));

const inResourceGroup = computed(() =>
    ['mods', 'packs', 'shaders', 'bridge', 'servers', 'recordings', 'projections'].includes(
        store.currentView
    )
);

// Route timings also respect the OS preference when Vue uses explicit timeout fallback.
const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');

const systemMotion = ref(motionQuery.matches);

const reducedMotion = computed(() => systemMotion.value || store.settings?.reduceMotion === true);

watch(
    reducedMotion,
    (value) => {
        document.documentElement.dataset.motion = value ? 'reduced' : 'full';
    },
    { immediate: true }
);

document.addEventListener('visibilitychange', () => {
    document.documentElement.dataset.visibility = document.hidden ? 'hidden' : 'visible';
});

const onMotionChange = () => {
    systemMotion.value = motionQuery.matches;
};

const routeDuration = computed(() => (reducedMotion.value ? 0 : { enter: 180, leave: 80 }));

const {
    navEl,
    bubbleStyle,
    selectionStyle,
    retarget: retargetNav,
    reset: resetNav,
    focusOut: navFocusOut,
    measure: measureNav
} = useNavigationBubble(
    computed(() => store.currentView),
    computed(() => [
        resourceExpanded.value,
        inResourceGroup.value,
        visibleNavItems.value,
        visibleResourceSubItems.value
    ])
);

/** 关闭启动器不影响游戏：游戏在跑时点关闭先提示一次，再真正关闭 */
let closeHintShown = false;

const win = async (action: 'minimize' | 'maximize' | 'close') => {
    if (action === 'close' && mascotRef.value) {
        try {
            await mascotRef.value.flush();
        } catch (error) {
            toast('互动次数尚未保存：' + errText(error), 'error');
            return;
        }
    }
    if (action === 'close' && store.launchState?.status === 'running' && !closeHintShown) {
        closeHintShown = true;
        toast('关闭启动器不影响游戏，游戏继续运行', 'info');
        setTimeout(() => sendIpc('window:close'), 1300);
        return;
    }
    sendIpc(`window:${action}`);
};

// ---------------- 启动器自更新弹窗 ----------------
const updateModal = reactive<{
    open: boolean;
    release: ReleaseInfo | null;
    state: 'found' | 'downloading' | 'done';
    taskId: string;
    slowHint: boolean;
    rollback: boolean;
}>({ open: false, release: null, state: 'found', taskId: '', slowHint: false, rollback: false });

/** 内测群号：设置覆盖优先，默认 shared/branding 常量 */
const qqGroup = computed(() => store.settings?.qqGroupNumber?.trim() || QQ_GROUP_NUMBER);

/** 更新下载任务进度（从下载中心任务列表取，含速度） */
const updateTask = computed(() => store.tasks.find((t) => t.id === updateModal.taskId));

const updatePercent = computed(() => updateTask.value?.progress ?? 0);

const updateSpeedText = computed(() =>
    updateTask.value?.speed ? formatSpeed(updateTask.value.speed) : ''
);

function openUpdateModal(release: ReleaseInfo, rollback = false) {
    updateModal.release = release;
    updateModal.state = 'found';
    updateModal.taskId = '';
    updateModal.slowHint = false;
    updateModal.rollback = rollback;
    updateModal.open = true;
}

// 设置页手动检查/任意页面写入 store.updatePrompt → 统一在此打开弹窗
watch(
    () => store.updatePrompt,
    (req) => {
        if (req) {
            openUpdateModal(req.release, req.rollback);
            store.updatePrompt = null;
        }
    }
);

async function onUpdateNow() {
    const release = updateModal.release;
    if (!release) return;
    try {
        const { taskId } = await startUpdateDownload(
            release,
            updateModal.rollback ? 'rollback' : 'upgrade'
        );
        updateModal.taskId = taskId;
        updateModal.state = 'downloading';
    } catch (e) {
        toast('启动更新下载失败：' + errText(e), 'error');
        updateModal.open = false;
    }
}

function onUpdateLater() {
    updateModal.open = false;
}

async function onUpdateSkip() {
    const release = updateModal.release;
    updateModal.open = false;
    if (!release) return;
    try {
        await skipUpdateVersion(release.version);
        store.settings = { ...store.settings!, skipUpdateVersion: release.version };
        toast(`已跳过 v${release.version}，下个版本再提醒`, 'info');
    } catch (e) {
        toast('保存失败：' + errText(e), 'error');
    }
}

async function onUpdateCancelDownload() {
    if (updateModal.taskId) await cancelTask(updateModal.taskId);
    updateModal.open = false;
}

async function onUpdateInstallNow() {
    const release = updateModal.release;
    if (!release) return;
    try {
        await applyUpdate(release);
        updateModal.open = false;
        toast(updateReadyMessage(), 'success');
    } catch (e) {
        toast('安装更新失败：' + errText(e), 'error');
        updateModal.open = false;
    }
}

// 配置文件版本不兼容（回退后旧版读到新版配置）：继续尝试 / 重置设置
const configMismatch = ref(false);

async function checkConfigStatus() {
    try {
        const s = await getConfigStatus();
        if (s.mismatch === 'newer') configMismatch.value = true;
    } catch {
        /* 检查失败不打扰 */
    }
}

async function onConfigReset() {
    try {
        await resetSettingsToDefaults();
        configMismatch.value = false;
        toast('设置已重置为默认值（原配置已备份）', 'success');
        setTimeout(() => location.reload(), 800);
    } catch (e) {
        toast('重置失败：' + errText(e), 'error');
    }
}

// 自绘标题栏中的返回按钮使用真实视图历史；不伪造“返回”入口。
const viewHistory = ref<ViewName[]>([]);

let navigatingBack = false;

watch(
    () => store.currentView,
    (next, previous) => {
        if (navigatingBack) {
            navigatingBack = false;
            return;
        }
        if (previous && previous !== next) {
            viewHistory.value = [...viewHistory.value.slice(-19), previous];
        }
    },
    { flush: 'sync' }
);

const canGoBack = computed(() => viewHistory.value.length > 0);

function goBack() {
    const target = viewHistory.value.at(-1);
    if (!target) return;
    viewHistory.value = viewHistory.value.slice(0, -1);
    navigatingBack = true;
    store.currentView = target;
}

// ---------------- 全局拖拽导入整合包 ----------------
const dragActive = ref(false);

/** 进入/离开子元素会成对触发 dragenter/dragleave，用计数器避免遮罩闪烁 */
let dragDepth = 0;

let internalDrag = false;

/** 最近一次 dragover 时间戳：浏览器对拖出窗口/异常手势会停发事件，超时兜底防覆盖层残留 */
let lastDragoverAt = 0;

let dragWatchdog: ReturnType<typeof setInterval> | null = null;

function endDrag() {
    internalDrag = false;
    dragDepth = 0;
    dragActive.value = false;
}

/** 覆盖层激活期间启动看门狗：1.5s 没有任何拖拽事件即强制复位 */
function startDragWatchdog() {
    stopDragWatchdog();
    lastDragoverAt = Date.now();
    dragWatchdog = setInterval(() => {
        if (dragActive.value && Date.now() - lastDragoverAt > 1500) endDrag();
    }, 400);
}

function stopDragWatchdog() {
    if (dragWatchdog) {
        clearInterval(dragWatchdog);
        dragWatchdog = null;
    }
}

watch(dragActive, (active) => (active ? startDragWatchdog() : stopDragWatchdog()));

const dragHasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes('Files');

const dragHasProviderText = (e: DragEvent) => {
    const types = Array.from(e.dataTransfer?.types ?? []);
    return types.includes('text/plain') || types.includes('text/uri-list');
};

const dragHasSupportedData = (e: DragEvent) =>
    acceptsImportDrag(Array.from(e.dataTransfer?.types ?? []), internalDrag);

const looksLikeYggdrasilProvider = (value: string): boolean => {
    const text = value.trim();
    return (
        /^authlib-injector:yggdrasil-server:/i.test(text) ||
        /^https?:\/\//i.test(text) ||
        /^\{[\s\S]*\}$/i.test(text) ||
        /(?:api\s*root|yggdrasil(?:\s*server)?)\s*[:=]/i.test(text) ||
        /^[\w.-]+\.[a-z]{2,}(?:[/:][^\s]*)?$/i.test(text)
    );
};

const resourceDropPage = () => ['keys', 'mods', 'packs', 'shaders'].includes(store.currentView);

function onDragEnter(e: DragEvent) {
    if (store.editMode) {
        e.preventDefault();
        e.stopImmediatePropagation();
        endDrag();
        return;
    }
    if (resourceDropPage()) {
        if (dragHasFiles(e)) e.preventDefault();
        endDrag();
        return;
    }
    if (!dragHasSupportedData(e)) return;
    e.preventDefault();
    dragDepth++;
    dragActive.value = showsImportOverlay(Array.from(e.dataTransfer?.types ?? []), internalDrag);
    if (dragActive.value) {
        dlOpen.value = false;
        noticeOpen.value = false;
    }
}

function onDragOver(e: DragEvent) {
    if (store.editMode) {
        e.preventDefault();
        e.stopImmediatePropagation();
        endDrag();
        return;
    }
    if (resourceDropPage()) {
        if (dragHasFiles(e)) e.preventDefault();
        endDrag();
        return;
    }
    if (!dragHasSupportedData(e)) return;
    e.preventDefault(); // 必须 preventDefault 才允许 drop
    lastDragoverAt = Date.now();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    dragActive.value = showsImportOverlay(Array.from(e.dataTransfer?.types ?? []), internalDrag);
}

function onDragLeave(e: DragEvent) {
    if (!dragHasSupportedData(e)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) dragActive.value = false;
}

function onDrop(e: DragEvent) {
    if (store.editMode) {
        e.preventDefault();
        e.stopImmediatePropagation();
        endDrag();
        return;
    }
    // MRPACK is unambiguously a whole pack, even on a local resource page.
    // Ordinary ZIP/JAR drops retain that page's existing batch import behavior.
    const mrpackDrop =
        dragHasFiles(e) &&
        Array.from(e.dataTransfer?.files ?? []).some((file) => /\.mrpack$/i.test(file.name));
    if (mrpackDrop) e.stopPropagation();
    if (resourceDropPage() && !mrpackDrop) {
        endDrag();
        e.preventDefault();
        e.stopPropagation();
        if (dragHasFiles(e)) {
            if (store.resourceDropHandler) store.resourceDropHandler(e);
            else toast('页面正在加载，请稍后再拖入', 'info');
        }
        return;
    }
    // 覆盖层复位先于一切判定：任何 drop 发生都意味着拖拽手势已结束
    dragDepth = 0;
    dragActive.value = false;
    if (!dragHasSupportedData(e)) return;
    e.preventDefault();
    dlOpen.value = false;
    noticeOpen.value = false;
    const dropped = Array.from(e.dataTransfer?.files ?? []);
    if (!dropped.length) {
        const text =
            e.dataTransfer?.getData('text/plain') || e.dataTransfer?.getData('text/uri-list') || '';
        if (looksLikeYggdrasilProvider(text)) {
            routeYggdrasilImport({ kind: 'text', value: text });
        }
        return;
    }
    const paths = dropped.map((f) => filePathFromDrop(f));
    const names = dropped.map((f) => f.name.toLowerCase());

    if (names.length === 1 && /\.(json|txt|url|yggdrasil)$/.test(names[0])) {
        routeYggdrasilImport({ kind: 'file', value: paths[0] });
        return;
    }

    // 单项拖入先按内容识别：.mrpack 始终优先，ZIP/文件夹可能是世界存档。
    if (names.length === 1) {
        void routeSingleImport(paths[0], names[0]);
        return;
    }
    // 全部为非压缩包扩展（.jar 或文件夹）→ MOD 拖入即装流程
    if (names.every((n) => !/\.(mrpack|zip)$/.test(n))) {
        modDrop.files = paths;
        modDrop.open = true;
        return;
    }
    toast('压缩包请一次导入一个文件；混合文件请分开拖入', 'error');
}

function routeYggdrasilImport(input: YggdrasilProviderInput) {
    store.pendingYggdrasilImport = input;
    store.currentView = 'accounts';
    void nextTick(() => {
        const pending = store.pendingYggdrasilImport;
        if (pending && store.yggdrasilImportHandler) {
            store.pendingYggdrasilImport = null;
            store.yggdrasilImportHandler(pending);
        }
    });
}

// ---------------- MOD 拖入即装 ----------------
const modDrop = reactive({ open: false, files: [] as string[] });

const worldModal = reactive({
    open: false,
    filePath: '',
    info: null as WorldImportInfo | null
});

let importProbeRevision = 0;

async function routeSingleImport(filePath: string, _displayName: string) {
    const revision = ++importProbeRevision;
    try {
        const result = await probeImport(filePath);
        if (revision !== importProbeRevision) return;
        if (result.kind === 'modpack') await openModpackImport(filePath, result.info);
        else if (result.kind === 'world')
            Object.assign(worldModal, { filePath, info: result.info, open: true });
        else if (result.kind === 'mod') Object.assign(modDrop, { files: [filePath], open: true });
        else toast(result.message, 'error');
    } catch (e) {
        if (revision === importProbeRevision) toast('导入识别失败：' + errText(e), 'error');
    }
}

// ---------------- 整合包导入确认弹窗 ----------------
const FORMAT_LABEL: Record<ModpackInfo['format'], string> = {
    mrpack: 'Modrinth (.mrpack)',
    curseforge: 'CurseForge',
    fullpack: '完整客户端包'
};

const FORMAT_TAG_CLASS: Record<ModpackInfo['format'], string> = {
    mrpack: 'tag-success',
    curseforge: 'tag-gold',
    fullpack: 'tag-cyan'
};

interface ModpackModal {
    open: boolean;
    probing: boolean;
    error: string;
    filePath: string;
    info: ModpackInfo | null;
    nameSource: 'file' | 'inner';
    customName: string;
    targetFolder: string;
    conflictAction: 'rename' | 'new' | 'update' | 'overwrite';
    existingId: string;
    confirmReplace: boolean;
    keySyncOverride: boolean;
}

const mpModal = reactive<ModpackModal>({
    open: false,
    probing: false,
    error: '',
    filePath: '',
    info: null,
    nameSource: 'file',
    customName: '',
    targetFolder: '',
    conflictAction: 'rename',
    existingId: '',
    confirmReplace: false,
    keySyncOverride: false
});

const mpFormatLabel = computed(() => (mpModal.info ? FORMAT_LABEL[mpModal.info.format] : ''));

const mpFormatTagClass = computed(() =>
    mpModal.info ? FORMAT_TAG_CLASS[mpModal.info.format] : ''
);

const normalizeMpName = (value: string) =>
    value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLocaleLowerCase('en-US');

const mpExistingInFolder = computed(() =>
    (mpModal.info?.existingInstances ?? []).filter((item) => item.folder === mpModal.targetFolder)
);

const mpNameConflict = computed(() =>
    mpExistingInFolder.value.find(
        (item) => normalizeMpName(item.id) === normalizeMpName(mpModal.customName)
    )
);

const mpRelatedExisting = computed(() => {
    const result = mpExistingInFolder.value.filter(
        (item) =>
            item.samePackVersion || normalizeMpName(item.id) === normalizeMpName(mpModal.customName)
    );
    return [...new Map(result.map((item) => [item.id, item])).values()];
});

const mpNeedsReplaceConfirm = computed(
    () => mpModal.conflictAction === 'update' || mpModal.conflictAction === 'overwrite'
);

function fmtPackBytes(bytes: number): string {
    if (!bytes) return '大小未知';
    if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
    return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function setMpNameSource(source: 'file' | 'inner') {
    mpModal.nameSource = source;
    if (mpModal.info)
        mpModal.customName = source === 'inner' ? mpModal.info.innerName : mpModal.info.fileName;
    mpModal.confirmReplace = false;
}

function onMpTargetFolderChange() {
    const related = mpRelatedExisting.value[0];
    mpModal.existingId = related?.id ?? '';
    mpModal.conflictAction = related ? 'new' : 'rename';
    mpModal.confirmReplace = false;
    mpModal.error = '';
}

function onMpConflictActionChange() {
    mpModal.confirmReplace = false;
    mpModal.error = '';
    if (mpNeedsReplaceConfirm.value) {
        const valid = mpExistingInFolder.value.some((item) => item.id === mpModal.existingId);
        if (!valid)
            mpModal.existingId =
                mpRelatedExisting.value[0]?.id ?? mpExistingInFolder.value[0]?.id ?? '';
    }
}

/** 拿到文件路径后先 probe 解析，弹确认框；解析失败在框内展示错误 */
async function openModpackImport(filePath: string, knownInfo?: ModpackInfo) {
    if (!filePath) return;
    Object.assign(mpModal, {
        open: true,
        probing: true,
        error: '',
        filePath,
        info: null,
        nameSource: 'file' as const,
        customName: '',
        targetFolder:
            store.settings?.folders.find((folder) => folder.isDefault)?.path ||
            store.settings?.activeFolder ||
            store.settings?.gameDir ||
            '',
        conflictAction: 'rename' as const,
        existingId: '',
        confirmReplace: false
    });
    try {
        const info = knownInfo ?? (await probeModpack(filePath));
        // 防止解析期间用户又发起了另一次导入，旧结果覆盖新弹窗
        if (mpModal.filePath === filePath) {
            mpModal.info = info;
            mpModal.customName = info.fileName;
            const relevant = info.existingInstances.find(
                (item) =>
                    item.folder === mpModal.targetFolder &&
                    (item.sameNormalizedName || item.samePackVersion)
            );
            mpModal.existingId = relevant?.id ?? '';
            mpModal.conflictAction = relevant ? 'new' : 'rename';
        }
    } catch (e) {
        if (mpModal.filePath === filePath) mpModal.error = errText(e);
    } finally {
        if (mpModal.filePath === filePath) mpModal.probing = false;
    }
}

function closeModpackImport() {
    mpModal.open = false;
}

/** 确认导入：主进程后台异步执行，完成/失败由 installDone 订阅统一提示 */
function confirmModpackImport() {
    if (!mpModal.info) return;
    if (!mpModal.customName.trim()) {
        mpModal.error = '实例名称不能为空';
        return;
    }
    if (mpModal.conflictAction === 'rename' && mpNameConflict.value) {
        mpModal.error = `实例名称已存在：${mpNameConflict.value.id}，请改名或选择其他处理方式`;
        return;
    }
    if (mpNeedsReplaceConfirm.value && (!mpModal.existingId || !mpModal.confirmReplace)) {
        mpModal.error = '请选择现有实例并勾选影响范围确认';
        return;
    }
    const filePath = mpModal.filePath;
    const nameSource = mpModal.nameSource;
    mpModal.open = false;
    toast('开始解析并安装整合包…', 'info');
    void installModpack(filePath, {
        nameSource,
        instanceName: mpModal.customName.trim(),
        targetFolder: mpModal.targetFolder,
        conflictAction: mpModal.conflictAction,
        existingId: mpModal.existingId || undefined,
        confirmReplace: mpNeedsReplaceConfirm.value && mpModal.confirmReplace,
        keySyncOverride: mpModal.keySyncOverride
    }).catch((e) => {
        toast('整合包安装失败：' + errText(e), 'error');
    });
}

/** 顶栏「导入」与拖入文件共用内容识别。 */
async function onImportClick() {
    try {
        const p = await selectFile();
        if (p) void routeSingleImport(p, p.split(/[\\/]/).pop() ?? p);
    } catch (e) {
        toast('导入失败：' + errText(e), 'error');
    }
}

// ---------------- 通知中心 ----------------
const noticeOpen = ref(false);

function toggleNotices() {
    noticeOpen.value = !noticeOpen.value;
    if (noticeOpen.value) markNoticesRead();
}

// ---------------- 启动失败日志导出 ----------------
const launchFail = reactive({ open: false, title: '', text: '', exporting: false });

async function onExportLogs() {
    if (launchFail.exporting) return;
    launchFail.exporting = true;
    try {
        const p = await exportLaunchLogs(store.launchingVersionId, store.launchingFolder);
        if (p) {
            toast(`错误日志已导出：${p}`, 'success');
            launchFail.open = false;
        }
    } catch (e) {
        toast('导出失败：' + errText(e), 'error');
    } finally {
        launchFail.exporting = false;
    }
}

// ---------------- 下载中心 ----------------
const dlOpen = ref(false);

const notesOpen = ref(false);

/** 顶栏空白处点击关闭已展开的下拉面板（顶栏是 -webkit-app-region:drag 拖拽区，点击不会落到下拉遮罩上） */
function onTopbarPointerDown(event: PointerEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest('button, input, select, textarea, a, [role="button"]')) closeTopDropdowns();
}

function closeTopDropdowns() {
    noticeOpen.value = false;
    dlOpen.value = false;
    notesOpen.value = false;
}

/** 操作习惯：下拉打开后点击面板与触发按钮之外的任意位置即关闭（全屏遮罩之外的兜底） */
function onGlobalPointerDown(event: PointerEvent) {
    if (!noticeOpen.value && !dlOpen.value && !notesOpen.value) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest?.('.notice-panel, .dl-toggle, [title="通知"]')) return;
    closeTopDropdowns();
}

const activeTaskCount = computed(
    () =>
        store.tasks.filter(
            (t) => t.status === 'running' || t.status === 'paused' || t.status === 'cancelling'
        ).length
);

const launcherHealth = computed(() => {
    if (store.launchState?.status === 'error') {
        return { tone: 'error', text: '最近启动出现异常' };
    }
    if (activeTaskCount.value) {
        return { tone: 'busy', text: `${activeTaskCount.value} 项后台任务进行中` };
    }
    return { tone: 'ok', text: '全部系统运行正常' };
});

async function onPauseTask(id: string) {
    const task = store.tasks.find((t) => t.id === id);
    if (!task || task.status !== 'running') return;
    try {
        if (await pauseTask(id)) task.status = 'paused';
    } catch (e) {
        toast('暂停失败：' + errText(e), 'error');
    }
}

async function onResumeTask(id: string) {
    const task = store.tasks.find((t) => t.id === id);
    if (!task || task.status !== 'paused') return;
    try {
        if (await resumeTask(id)) task.status = 'running';
    } catch (e) {
        toast('恢复失败：' + errText(e), 'error');
    }
}

async function onCancelTask(id: string) {
    const task = store.tasks.find((t) => t.id === id);
    if (!task || (task.status !== 'running' && task.status !== 'paused')) return;
    task.status = 'cancelling';
    try {
        const found = await cancelTask(id);
        if (!found) {
            finalizeTask({ taskId: id, ok: false, cancelled: true, error: '任务已结束' });
            toast('任务已结束或不存在', 'info');
        }
    } catch (e) {
        toast('取消失败：' + errText(e), 'error');
    }
}

/** 任务副标题：阶段标签与进度文本重复时只显示一次 */
function taskSubText(t: { stage: string; text: string }): string {
    if (t.stage === 'parallel') return '';
    const label = stageLabel(t.stage);
    return t.text.startsWith(label) ? t.text : `${label} · ${t.text}`;
}

function taskEtaText(seconds?: number): string {
    if (seconds == null || !Number.isFinite(seconds) || seconds <= 3) return '';
    if (seconds >= 3600) return ` · 本阶段约剩 ${Math.ceil(seconds / 3600)}h`;
    if (seconds >= 60) return ` · 本阶段约剩 ${Math.ceil(seconds / 60)}min`;
    return ` · 本阶段约剩 ${Math.round(seconds)}s`;
}

function fmtNoticeTime(ts: number): string {
    const d = new Date(ts);
    const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    const today = new Date().toDateString() === d.toDateString();
    return today ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
}

/** 背景图列表：多图自动切换用 images；否则回退单张 image */
const bgImages = computed(() => {
    const bg = appearancePreview.value?.background;
    if (!bg || bg.mode !== 'image') return [] as string[];
    return bg.images?.length ? bg.images : bg.image ? [bg.image] : [];
});
const BG_INDEX_KEY = 'kamucl:bg-last-index';
const currentBgIndex = ref(0);
const currentBgImage = computed(
    () => bgImages.value[currentBgIndex.value % Math.max(1, bgImages.value.length)] ?? ''
);

/** 切换到下一张背景图（按顺序/随机）；off 模式不切换 */
function switchBackground() {
    const list = bgImages.value;
    const mode = appearancePreview.value?.background.switchMode ?? 'off';
    if (mode === 'off' || list.length < 2) return;
    if (mode === 'random') {
        let next = currentBgIndex.value;
        while (next === currentBgIndex.value) next = Math.floor(Math.random() * list.length);
        currentBgIndex.value = next;
    } else {
        currentBgIndex.value = (currentBgIndex.value + 1) % list.length;
    }
    localStorage.setItem(BG_INDEX_KEY, String(currentBgIndex.value));
}

let bgSwitchTimer: ReturnType<typeof setInterval> | undefined;
function armBgSwitchTimer() {
    clearInterval(bgSwitchTimer);
    bgSwitchTimer = undefined;
    const bg = appearancePreview.value?.background;
    const mode = bg?.switchMode ?? 'off';
    if (bg?.mode !== 'image' || mode === 'off' || bgImages.value.length < 2) return;
    const sec = Math.max(30, bg?.switchIntervalSec ?? 300);
    // document.hidden 时跳过切换：页面不可见即暂停轮换，回到前台后下一拍继续
    bgSwitchTimer = setInterval(() => {
        if (document.hidden) return;
        switchBackground();
    }, sec * 1000);
}

const failedBackground = ref('');

watch(
    () => [appearancePreview.value?.background.mode, currentBgImage.value] as const,
    ([mode, imagePath]) => {
        failedBackground.value = '';
        if (mode !== 'image' || !imagePath) return;
        const probe = new Image();
        probe.onload = () => {
            if (currentBgImage.value === imagePath) failedBackground.value = '';
        };
        probe.onerror = () => {
            if (currentBgImage.value !== imagePath) return;
            failedBackground.value = imagePath;
            toast('自定义背景不可用，已回退到主题默认背景', 'error');
        };
        probe.src = managedImageUrl(imagePath);
    },
    { immediate: true }
);

// 切换策略/图片列表变化时重排定时器；图片被删除导致越界时收敛索引
watch(
    [
        () => appearancePreview.value?.background.switchMode,
        () => appearancePreview.value?.background.switchIntervalSec,
        bgImages
    ],
    () => {
        if (currentBgIndex.value >= bgImages.value.length) currentBgIndex.value = 0;
        armBgSwitchTimer();
    }
);

/**
 * 用户显式选择的个性化背景层。默认不生成内部壁纸：桌面透视由透明
 * BrowserWindow + Windows DWM Acrylic 提供。
 */
const bgStyle = computed(() => {
    const bg = appearancePreview.value?.background;
    if (!bg || bg.mode === 'none') return null;
    if (bg.mode === 'color') {
        return {
            background: bg.color,
            opacity: String(bg.opacity)
        };
    }
    if (
        bg.mode === 'image' &&
        currentBgImage.value &&
        failedBackground.value !== currentBgImage.value
    ) {
        const size = bg.fit === 'fill' ? '100% 100%' : bg.fit === 'fit' ? 'contain' : 'cover';
        return {
            inset: bg.blur > 0 ? `${-Math.ceil(bg.blur * 1.5)}px` : '0',
            backgroundColor: bg.color,
            backgroundImage: `url("${managedImageUrl(currentBgImage.value)}")`,
            backgroundSize: size,
            backgroundPosition: 'center',
            backgroundRepeat: 'no-repeat',
            ...backgroundImageEffect(bg)
        };
    }
    return null;
});

// ---------------- 个性化点选编辑模式 ----------------
function onEditKeydown(e: KeyboardEvent) {
    if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'e') {
        e.preventDefault();
        void enterEditMode();
    }
}

// ---------------- 初始化与事件订阅 -

const offs: Array<() => void> = [];

onMounted(async () => {
    void pollTickets();
    offs.push(installAppearanceRuntime(() => appearancePreview.value));
    offs.push(installVisualDesign());
    void loadExitNotices();
    motionQuery.addEventListener('change', onMotionChange);
    // 每次上线自动切换一张背景图（按顺序/随机）；off 模式固定第一张
    {
        const mode = appearancePreview.value?.background.switchMode ?? 'off';
        if (
            appearancePreview.value?.background.mode === 'image' &&
            mode !== 'off' &&
            bgImages.value.length > 1
        ) {
            const last = Number(localStorage.getItem(BG_INDEX_KEY) ?? -1);
            currentBgIndex.value =
                Number.isInteger(last) && last >= 0 && last < bgImages.value.length ? last : 0;
            switchBackground();
        }
        armBgSwitchTimer();
    }
    void import('../../plugins').then((m) => m.loadEnabledPlugins());
    window.addEventListener('keydown', onEditKeydown);
    // Teleports are outside .shell; capture above their masks without accepting
    // internal text/image drags or invoking the import handler twice.
    const dragStart = () => {
        internalDrag = true;
    };
    const dragEvents = {
        dragenter: onDragEnter,
        dragover: onDragOver,
        dragleave: onDragLeave,
        drop: onDrop,
        dragstart: dragStart,
        dragend: endDrag
    };
    for (const [type, listener] of Object.entries(dragEvents)) {
        window.addEventListener(type, listener as EventListener, true);
        offs.push(() => window.removeEventListener(type, listener as EventListener, true));
    }
    // 所有通用导入入口使用同一内容分类，避免整合包内的存档抢占流程。
    store.importHandler = (filePath: string) =>
        void routeSingleImport(filePath, filePath.split(/[\\/]/).pop() ?? filePath);
    window.addEventListener('pointerdown', onGlobalPointerDown, true);
    offs.push(() => window.removeEventListener('pointerdown', onGlobalPointerDown, true));
    offs.push(
        subscribeIpc('window:caption-pointerdown', () => closeTopDropdowns()),
        subscribeIpc('files:dragError', (error) =>
            toast('无法拖出文件：' + String(error), 'error')
        ),
        onProgress((e) => {
            if (
                e.manualFiles &&
                !store.tasks.some((t) => t.manualFiles?.token === e.manualFiles?.token)
            )
                dlOpen.value = true;
            store.progress = e;
            if (e.versionId) store.installProgress[e.versionId] = e;
            upsertTaskProgress(e);
        }),
        onTaskDone((r) => {
            finalizeTask(r);
            resetProgressMono(r.taskId);
            // 更新下载完成 → 弹窗切到「下载完成」待安装态
            if (updateModal.open && updateModal.taskId && r.taskId === updateModal.taskId) {
                if (r.ok) {
                    updateModal.state = 'done';
                } else {
                    updateModal.open = false;
                    if (!r.cancelled) toast('更新下载失败：' + (r.error || '未知错误'), 'error');
                }
            }
            if (r.cancelled) toast('任务已取消', 'info');
        }),
        onUpdatePrompt((payload) => {
            // 回滚通知（更新失败自动还原后备份）
            if ((payload as { rollbackNotice?: boolean }).rollbackNotice) {
                toast(
                    '上次更新未完成，已停止自动重试。可在设置中重新下载或选择备份恢复。',
                    'error'
                );
                return;
            }
            store.updatePrompt = { release: payload, rollback: false };
        }),
        onUpdateSlowHint((r) => {
            if (updateModal.open && r.taskId === updateModal.taskId) updateModal.slowHint = true;
        }),
        onUpdateReady((r) => {
            toast(updateReadyMessage(`新版本 v${r.version} 已下载完成`), 'success');
        }),
        onInstallDone((r) => {
            store.installing.delete(r.versionId);
            delete store.installProgress[r.versionId];
            if (store.progress?.taskId === r.taskId) store.progress = null;
            finalizeTask(r);
            resetProgressMono(r.taskId);
            if (r.ok) {
                store.failedInstalls.delete(r.versionId);
                const favorites = r.favoriteModsResult;
                toast(
                    favorites
                        ? `版本 ${r.versionId} 安装完成；${favorites.baseOnly ? '已按确认不安装收藏模组' : `已安装 ${favorites.installed} 项收藏模组及 ${favorites.dependencies} 项必要前置，跳过 ${favorites.skipped.length} 项`}。模组目录：${favorites.modsDirectory}`
                        : `版本 ${r.versionId} 安装完成`,
                    'success'
                );
                void refreshInstalled();
            } else if (r.cancelled) {
                // 用户主动取消：不记失败、不弹错误（taskDone 已提示）
                void refreshInstalled();
            } else {
                store.failedInstalls.add(r.versionId);
                toast(
                    `安装失败${r.stage ? `（${stageLabel(r.stage)}）` : ''}：` +
                        (r.error ?? '未知错误'),
                    'error'
                );
            }
        }),
        // 游戏目录迁移完成：立即全局刷新（版本列表/最近游戏/资源管理），全程无需重启
        onGameDirDone((r) => {
            store.progress = null;
            resetProgressMono();
            if (r.ok) {
                void refreshInstalled().then(() => {
                    store.fsRefreshTick++;
                });
            }
        }),
        onLaunchLog((line) => {
            store.logs.push(line);
            if (store.logs.length > 1000) store.logs.splice(0, store.logs.length - 1000);
        }),
        onLaunchState((s) => {
            const focused = applyLaunchState(s);
            // 启动成功（进入 running）时记录该版本的最近游玩时间
            if (s.status === 'running' && store.launchingVersionId) {
                recordLastPlayed(s.versionId || store.launchingVersionId);
            }
            if (focused && (s.status === 'exited' || s.status === 'error')) store.progress = null;
            if (s.status === 'error') {
                // 启动失败：弹窗提示并提供「导出错误日志」
                launchFail.open = true;
                launchFail.title = '游戏启动失败';
                launchFail.text = s.text;
            } else if (s.status === 'exited') {
                if (
                    s.savedWindowSize?.scope === 'global' &&
                    store.settings &&
                    JSON.stringify(store.settings.resolution) ===
                        JSON.stringify(s.savedWindowSize.previous)
                ) {
                    store.settings.resolution = { ...s.savedWindowSize.resolution };
                }
                if (s.savedWindowSize?.scope === 'instance')
                    void refreshInstalled().catch(() => undefined);
                if (shouldReportGameCrash(s)) {
                    launchFail.open = true;
                    launchFail.title = `游戏异常退出（代码 ${signedExitCode(s.code ?? null) ?? '未知'}）`;
                    launchFail.text =
                        '游戏进程崩溃或被异常终止。可导出错误日志（含 crash-report 与 latest.log）用于排查。';
                } else {
                    toast(s.exitKind === 'shutdown-timeout' ? s.text : '游戏已退出', 'info');
                }
                // 游戏退出后只扫描刚运行的实例，避免共享 servers.dat 被错误关联到其他版本。
                const exitedVersionId = s.versionId || store.launchingVersionId;
                const exitedFolder =
                    s.folder ||
                    store.launchingFolder ||
                    (store.settings?.activeFolder ?? store.settings?.gameDir);
                void import('../../api').then(({ syncServersFromDat }) =>
                    syncServersFromDat(exitedVersionId || undefined, exitedFolder).catch(
                        () => undefined
                    )
                );
            }
        })
    );

    try {
        store.settings = await getSettings();
        sendIpc('boot:stage', 'settings');
        void checkConfigStatus();
        await Promise.all([
            refreshAccounts().then(() => sendIpc('boot:stage', 'accounts')),
            refreshInstalled().then(() => sendIpc('boot:stage', 'instances'))
        ]);
        // 启动自检：发现上次下载未完成的残缺版本，提示去已安装页处理
        const broken = store.installed.filter((v) => v.incomplete);
        if (broken.length) {
            toast(
                `检测到 ${broken.length} 个版本下载未完成（${broken.map((b) => b.id).join('、')}），可在「游戏版本 → 已安装」继续下载或删除残留`,
                'info'
            );
        }
    } catch (e) {
        toast('初始化失败：' + errText(e), 'error');
    } finally {
        store.initialized = true;
        await nextTick();
        await waitForBootTasks();
        await nextTick();
        await document.fonts.ready;
        // Decode rendered assets after async account/skin/thumbnail bindings have settled.
        await Promise.all([...document.images].map((img) => img.decode().catch(() => undefined)));
        await nextTick();
        await waitForBootTasks();
        sealBootTasks();
        sendIpc('boot:stage', 'assets');
        await new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
        );
        sendIpc('boot:stage', 'paint');
        sendIpc('boot:renderer-ready');
    }
});

onUnmounted(() => {
    motionQuery.removeEventListener('change', onMotionChange);
    stopDragWatchdog();
    clearInterval(bgSwitchTimer);
    window.removeEventListener('keydown', onEditKeydown);
    offs.forEach((off) => off());
});

    return { isMac, updateReadyMessage, InstanceCenter, GameView, ModsView, RecordingsView, ProjectionsView, MascotStage, mascotOpen, mascotReady, mascotBusy, mascotSoftware, mascotRef, mascotKeyboard, openMascots, closeMascots, PacksView, ShadersView, KeysView, BridgeView, SkinsView, CommunityView, ServersView, FriendConnectView, SettingsView, AccountsView, appVersion, viewMap, currentComponent, navItems, isFeatureOff, visibleNavItems, visibleResourceSubItems, resourceSubItems, resourceExpanded, inResourceGroup, motionQuery, systemMotion, reducedMotion, onMotionChange, routeDuration, navEl, bubbleStyle, selectionStyle, retargetNav, resetNav, navFocusOut, measureNav, closeHintShown, win, updateModal, qqGroup, updateTask, updatePercent, updateSpeedText, openUpdateModal, onUpdateNow, onUpdateLater, onUpdateSkip, onUpdateCancelDownload, onUpdateInstallNow, configMismatch, checkConfigStatus, onConfigReset, viewHistory, navigatingBack, canGoBack, goBack, dragActive, dragDepth, internalDrag, lastDragoverAt, dragWatchdog, endDrag, startDragWatchdog, stopDragWatchdog, dragHasFiles, dragHasProviderText, dragHasSupportedData, looksLikeYggdrasilProvider, resourceDropPage, onDragEnter, onDragOver, onDragLeave, onDrop, routeYggdrasilImport, modDrop, worldModal, importProbeRevision, routeSingleImport, FORMAT_LABEL, FORMAT_TAG_CLASS, mpModal, mpFormatLabel, mpFormatTagClass, normalizeMpName, mpExistingInFolder, mpNameConflict, mpRelatedExisting, mpNeedsReplaceConfirm, fmtPackBytes, setMpNameSource, onMpTargetFolderChange, onMpConflictActionChange, openModpackImport, closeModpackImport, confirmModpackImport, onImportClick, noticeOpen, toggleNotices, launchFail, onExportLogs, dlOpen, notesOpen, onTopbarPointerDown, closeTopDropdowns, onGlobalPointerDown, activeTaskCount, launcherHealth, onPauseTask, onResumeTask, onCancelTask, taskSubText, taskEtaText, fmtNoticeTime, failedBackground, bgImages, BG_INDEX_KEY, currentBgIndex, currentBgImage, switchBackground, bgSwitchTimer, armBgSwitchTimer, bgStyle, onEditKeydown, offs, instanceCenter, brandHead, designStageReady, CreatorMotto, Toasts, UpdateModal, ModpackSupplement };
}

export type AppShellContext = ReturnType<typeof useAppShell>;
export const APP_SHELL_CONTEXT = Symbol('app-shell-context') as InjectionKey<AppShellContext>;
