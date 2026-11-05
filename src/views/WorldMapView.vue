<script setup lang="ts">
/**
 * 世界 2D 地图：把实例里真实存档的 region/*.mca 渲染成瓦片金字塔并展示。
 * 与种子地图互补——那边按种子推演生成结果，这边看实际玩过的地形。
 * 后端 src-tauri/src/world_map.rs：瓦片 PNG 落盘缓存 + 多核并行解析，
 * 前端只做视口内瓦片懒加载（按到视口中心距离排序）与缩放回退绘制。
 * 注意：瓦片经 kamucl-asset 协议加载必须用 <img>（WebView2 下 fetch 拉自定义协议会
 * Failed to fetch，截图页同款限制），不能 fetch + createImageBitmap。
 */
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { IPC } from '@shared/types';
import type { WorldMapDimensionId, WorldMapInfo, WorldMapTileResult } from '@shared/worldMap';
import { managedImageUrl } from '../managedAssets';
import ResourcePathChip from '../components/ResourcePathChip.vue';
import SelectMenu from '../components/SelectMenu.vue';
import { store } from '../store';
import {
    list as listInstances,
    get_instance_worlds,
    type GameInstance,
    type SingleplayerWorld
} from '../lab/instance';

const MAX_LEVEL = 8;
/** 每张瓦片 512×512；LRU 上限 80 张 */
const TILE_CACHE_LIMIT = 80;
const TILE_CONCURRENCY = 12;
const REFRESH_DEBOUNCE = 80;
const MIN_SCALE = 1 / 16;
const MAX_SCALE = 8192;
const DIMENSIONS: Array<{ id: WorldMapDimensionId; label: string }> = [
    { id: 'overworld', label: '主世界' },
    { id: 'nether', label: '下界' },
    { id: 'end', label: '末地' }
];
const STORAGE_KEY = 'kamucl.worldmap';

/** 通道名一律取自 @shared/types 的 IPC 常量（与 ScreenshotsView 同款局部 shim） */
const invoke = <T,>(channel: string, ...args: unknown[]) =>
    window.kamucl.invoke(channel, ...args) as Promise<T>;

type TileJob = { dim: WorldMapDimensionId; level: number; tx: number; tz: number };

const instances = ref<GameInstance[]>([]);
const worlds = ref<SingleplayerWorld[]>([]);
const instanceId = ref('');
const worldPath = ref('');
const dimension = ref<WorldMapDimensionId>('overworld');
const info = ref<WorldMapInfo | null>(null);
const infoLoading = ref(false);
const infoError = ref('');
/** 瓦片层面失败计数与首条错误（单张失败不炸整页，状态条给重试入口） */
const tileErrors = ref(0);
const tileErrorMessage = ref('');

const stage = ref<HTMLElement>();
const canvas = ref<HTMLCanvasElement>();
const viewport = reactive({ width: 0, height: 0 });
const center = reactive({ x: 0, z: 0 });
/** 每像素对应多少方块（1 = 1px/方块；>1 为缩小，<1 为放大） */
const scale = ref(2);
/** 缩小上限随世界范围走（适配后视野的 2.5 倍），避免无限缩小触发超大范围建图 */
const maxViewScale = ref(MAX_SCALE);
const cursorBlock = reactive({ x: 0, z: 0, inside: false });
const tileLoading = ref(0);
const dragging = ref(false);

const tiles = new Map<string, HTMLImageElement>();
const pending = new Map<string, true>();
const emptyTiles = new Set<string>();
const tileQueue: TileJob[] = [];
const levelKeyMemo = new Map<number, Set<string>>();
let activeRequests = 0;
let refreshTimer: number | undefined;
let redrawFrame: number | undefined;
let resizeObserver: ResizeObserver | undefined;
let persistTimer: number | undefined;
let lastLoadedKey = '';

const instanceOptions = computed(() =>
    instances.value.map((instance) => ({ value: instance.id, label: instance.id }))
);
const worldOptions = computed(() =>
    worlds.value.map((world) => ({ value: world.path, label: world.name }))
);
const dimensionRegions = computed(() => info.value?.dimensions[dimension.value] ?? []);
const dimensionEmpty = (id: WorldMapDimensionId) =>
    !!info.value && (info.value.dimensions[id]?.length ?? 0) === 0;

function currentLevel(): number {
    return Math.min(MAX_LEVEL, Math.max(0, Math.floor(Math.log2(Math.max(scale.value, 1 / 64)))));
}

function tileSpanBlocks(level: number): number {
    return 512 * 2 ** level;
}

function tileKeyOf(job: { dim: WorldMapDimensionId; level: number; tx: number; tz: number }) {
    return `${job.dim}:${job.level}:${job.tx}:${job.tz}`;
}

/** 该级别下实际存在数据的瓦片集合（由 region 清单折叠而来，负坐标算术右移=向下取整） */
function levelKeys(level: number): Set<string> {
    const cached = levelKeyMemo.get(level);
    if (cached) return cached;
    const keys = new Set<string>();
    for (const region of dimensionRegions.value) {
        keys.add(`${region.x >> level}_${region.z >> level}`);
    }
    levelKeyMemo.set(level, keys);
    return keys;
}

function resetTileCaches() {
    tiles.clear();
    pending.clear();
    emptyTiles.clear();
    levelKeyMemo.clear();
    tileQueue.length = 0;
    tileErrors.value = 0;
    tileErrorMessage.value = '';
}

// ---------------- 信息加载 ----------------

async function loadInstances() {
    instances.value = await listInstances();
    if (!instanceId.value || !instances.value.some((i) => i.id === instanceId.value)) {
        instanceId.value = instances.value[0]?.id ?? '';
    }
}

async function loadWorlds() {
    worlds.value = [];
    worldPath.value = '';
    if (!instanceId.value) return;
    worlds.value = (
        await get_instance_worlds({ id: instanceId.value } as GameInstance)
    ).map((world) => ({
        path: world.path,
        name: world.name,
        icon: world.icon,
        last_played: world.last_played
    }));
    if (!worldPath.value || !worlds.value.some((w) => w.path === worldPath.value)) {
        worldPath.value = worlds.value[0]?.path ?? '';
    }
}

async function loadInfo(keepView = false) {
    const loadKey = `${instanceId.value}\u{0}${worldPath.value}\u{0}${dimension.value}`;
    if (!instanceId.value || !worldPath.value) {
        info.value = null;
        infoError.value = '';
        resetTileCaches();
        return;
    }
    infoLoading.value = true;
    infoError.value = '';
    try {
        const next = await invoke<WorldMapInfo>(IPC.worldMapInfo, {
            instanceId: instanceId.value,
            world: worldPath.value
        });
        if (loadKey !== `${instanceId.value}\u{0}${worldPath.value}\u{0}${dimension.value}`) return;
        info.value = next;
        resetTileCaches();
        if (keepView && lastLoadedKey === loadKey) {
            scheduleRefresh();
        } else {
            fitView();
            lastLoadedKey = loadKey;
        }
    } catch (error) {
        if (loadKey === `${instanceId.value}\u{0}${worldPath.value}\u{0}${dimension.value}`) {
            info.value = null;
            infoError.value = error instanceof Error ? error.message : String(error);
        }
    } finally {
        infoLoading.value = false;
    }
}

// ---------------- 视图几何 ----------------

function blockToScreen(x: number, z: number) {
    return {
        x: viewport.width / 2 + (x - center.x) / scale.value,
        y: viewport.height / 2 + (z - center.z) / scale.value
    };
}

function screenToBlock(x: number, y: number) {
    return {
        x: center.x + (x - viewport.width / 2) * scale.value,
        z: center.z + (y - viewport.height / 2) * scale.value
    };
}

/** 让全部 region 恰好可见（留 16% 边距） */
function fitView() {
    const regions = dimensionRegions.value;
    if (!regions.length || !viewport.width) {
        maxViewScale.value = MAX_SCALE;
        return;
    }
    let minX = Infinity;
    let minZ = Infinity;
    let maxX = -Infinity;
    let maxZ = -Infinity;
    for (const region of regions) {
        minX = Math.min(minX, region.x * 512);
        minZ = Math.min(minZ, region.z * 512);
        maxX = Math.max(maxX, (region.x + 1) * 512);
        maxZ = Math.max(maxZ, (region.z + 1) * 512);
    }
    center.x = (minX + maxX) / 2;
    center.z = (minZ + maxZ) / 2;
    const spanX = Math.max(maxX - minX, 1);
    const spanZ = Math.max(maxZ - minZ, 1);
    scale.value = Math.min(
        MAX_SCALE,
        Math.max(MIN_SCALE, Math.max(spanX / viewport.width, spanZ / viewport.height) * 1.16)
    );
    maxViewScale.value = Math.max(scale.value * 2.5, MIN_SCALE);
    scheduleRefresh();
}

// ---------------- 瓦片调度（懒加载：只取视口内、近处优先） ----------------

function scheduleRefresh(delay = REFRESH_DEBOUNCE) {
    if (!viewport.width || !viewport.height) return;
    if (refreshTimer !== undefined) window.clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(requestVisibleTiles, delay);
}

function requestVisibleTiles() {
    refreshTimer = undefined;
    if (!info.value?.supported || !dimensionRegions.value.length) return;
    // 视口/级别变化后旧队列整体作废，别让旧级别任务和新级别抢核
    tileQueue.length = 0;
    const level = currentLevel();
    const span = tileSpanBlocks(level);
    const halfW = (viewport.width / 2) * scale.value;
    const halfH = (viewport.height / 2) * scale.value;
    const minTx = Math.floor((center.x - halfW) / span);
    const maxTx = Math.floor((center.x + halfW) / span);
    const minTz = Math.floor((center.z - halfH) / span);
    const maxTz = Math.floor((center.z + halfH) / span);
    const existing = levelKeys(level);
    const candidates: TileJob[] = [];
    for (let tx = minTx; tx <= maxTx; tx++) {
        for (let tz = minTz; tz <= maxTz; tz++) {
            if (!existing.has(`${tx}_${tz}`)) continue;
            const job = { dim: dimension.value, level, tx, tz };
            const key = tileKeyOf(job);
            if (tiles.has(key) || pending.has(key) || emptyTiles.has(key)) continue;
            candidates.push(job);
        }
    }
    // 插队渲染：按离视口中心由近到远排序，用户窗口内的内容先出图
    candidates.sort(
        (a, b) =>
            Math.hypot((a.tx + 0.5) * span - center.x, (a.tz + 0.5) * span - center.z) -
            Math.hypot((b.tx + 0.5) * span - center.x, (b.tz + 0.5) * span - center.z)
    );
    tileQueue.push(...candidates);
    pumpTileQueue();
}

function pumpTileQueue() {
    while (activeRequests < TILE_CONCURRENCY && tileQueue.length) {
        const job = tileQueue.shift();
        if (!job) return;
        const key = tileKeyOf(job);
        if (tiles.has(key) || pending.has(key)) continue;
        pending.set(key, true);
        activeRequests += 1;
        tileLoading.value = activeRequests;
        void loadTile(job, key).finally(() => {
            activeRequests -= 1;
            pending.delete(key);
            tileLoading.value = activeRequests;
            pumpTileQueue();
        });
    }
}

async function loadTile(job: TileJob, key: string) {
    try {
        const result = await invoke<WorldMapTileResult>(IPC.worldMapRenderTile, {
            instanceId: instanceId.value,
            world: worldPath.value,
            dimension: job.dim,
            level: job.level,
            tx: job.tx,
            tz: job.tz
        });
        if ('empty' in result) {
            emptyTiles.add(key);
            return;
        }
        if (job.dim !== dimension.value) return;
        // kamucl-asset 自定义协议在 WebView2 下只能走 <img>（fetch 会失败）
        const image = new Image();
        image.decoding = 'async';
        image.src = managedImageUrl(result.path);
        await image.decode();
        if (job.dim !== dimension.value) return;
        tiles.delete(key);
        tiles.set(key, image);
        while (tiles.size > TILE_CACHE_LIMIT) {
            const oldest = tiles.keys().next().value as string | undefined;
            if (oldest === undefined) break;
            tiles.delete(oldest);
        }
        redraw();
    } catch (error) {
        if (job.dim === dimension.value) {
            tileErrors.value += 1;
            if (!tileErrorMessage.value) {
                tileErrorMessage.value = error instanceof Error ? error.message : String(error);
            }
        }
    }
}

function retryTiles() {
    tileErrors.value = 0;
    tileErrorMessage.value = '';
    scheduleRefresh(0);
}

// ---------------- 绘制 ----------------

function redraw() {
    if (redrawFrame !== undefined) return;
    redrawFrame = window.requestAnimationFrame(() => {
        redrawFrame = undefined;
        drawMap();
    });
}

function drawCheckerboard(context: CanvasRenderingContext2D) {
    const style = getComputedStyle(canvas.value!);
    const surface2 = style.getPropertyValue('--surface-2').trim() || '#17191D';
    const surface3 = style.getPropertyValue('--surface-3').trim() || '#23262D';
    context.fillStyle = surface2;
    context.fillRect(0, 0, viewport.width, viewport.height);
    context.fillStyle = surface3;
    for (let y = 0; y < viewport.height; y += 32) {
        for (let x = 0; x < viewport.width; x += 32) {
            if ((x / 32 + y / 32) % 2 === 0) context.fillRect(x, y, 32, 32);
        }
    }
}

function drawMap() {
    const element = canvas.value;
    const context = element?.getContext('2d');
    if (!element || !context || !viewport.width || !viewport.height) return;
    drawCheckerboard(context);
    if (!info.value?.supported) return;

    const level = currentLevel();
    context.imageSmoothingEnabled = false;
    // 先画更低级别的祖先瓦片兜底，再覆盖当前级别的清晰瓦片
    for (let fallback = level - 1; fallback >= 0; fallback--) {
        drawLevelTiles(context, fallback);
    }
    drawLevelTiles(context, level);
    drawSpawn(context);
}

function drawLevelTiles(context: CanvasRenderingContext2D, level: number) {
    const span = tileSpanBlocks(level);
    const halfW = (viewport.width / 2) * scale.value;
    const halfH = (viewport.height / 2) * scale.value;
    const minTx = Math.floor((center.x - halfW) / span);
    const maxTx = Math.floor((center.x + halfW) / span);
    const minTz = Math.floor((center.z - halfH) / span);
    const maxTz = Math.floor((center.z + halfH) / span);
    const display = span / scale.value;
    for (let tx = minTx; tx <= maxTx; tx++) {
        for (let tz = minTz; tz <= maxTz; tz++) {
            const image = tiles.get(`${dimension.value}:${level}:${tx}:${tz}`);
            if (!image) continue;
            const position = blockToScreen(tx * span, tz * span);
            context.drawImage(image, position.x, position.y, display, display);
        }
    }
}

function drawSpawn(context: CanvasRenderingContext2D) {
    if (!info.value || dimension.value !== 'overworld') return;
    const position = blockToScreen(info.value.spawn.x + 0.5, info.value.spawn.z + 0.5);
    if (
        position.x < -20 ||
        position.y < -20 ||
        position.x > viewport.width + 20 ||
        position.y > viewport.height + 20
    ) {
        return;
    }
    context.save();
    context.fillStyle = '#F5B342';
    context.strokeStyle = 'rgba(0,0,0,.55)';
    context.lineWidth = 1.5;
    context.beginPath();
    context.moveTo(position.x, position.y - 7);
    context.lineTo(position.x + 6, position.y);
    context.lineTo(position.x, position.y + 7);
    context.lineTo(position.x - 6, position.y);
    context.closePath();
    context.fill();
    context.stroke();
    context.restore();
}

// ---------------- 交互 ----------------

function applyZoom(nextScale: number, pivotX: number, pivotY: number) {
    const clamped = Math.min(maxViewScale.value, Math.max(MIN_SCALE, nextScale));
    const pivot = screenToBlock(pivotX, pivotY);
    scale.value = clamped;
    const after = screenToBlock(pivotX, pivotY);
    center.x += pivot.x - after.x;
    center.z += pivot.z - after.z;
    redraw();
    scheduleRefresh();
    schedulePersist();
}

function onWheel(event: WheelEvent) {
    event.preventDefault();
    const rect = canvas.value!.getBoundingClientRect();
    const factor = event.deltaY > 0 ? 1.25 : 0.8;
    applyZoom(scale.value * factor, event.clientX - rect.left, event.clientY - rect.top);
}

let pointerLast: { x: number; y: number } | null = null;

function onPointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    dragging.value = true;
    pointerLast = { x: event.clientX, y: event.clientY };
    canvas.value?.setPointerCapture(event.pointerId);
}

function onPointerMove(event: PointerEvent) {
    const rect = canvas.value!.getBoundingClientRect();
    const world = screenToBlock(event.clientX - rect.left, event.clientY - rect.top);
    cursorBlock.x = Math.floor(world.x);
    cursorBlock.z = Math.floor(world.z);
    cursorBlock.inside = true;
    if (!dragging.value || !pointerLast) return;
    center.x -= (event.clientX - pointerLast.x) * scale.value;
    center.z -= (event.clientY - pointerLast.y) * scale.value;
    pointerLast = { x: event.clientX, y: event.clientY };
    redraw();
    scheduleRefresh();
    schedulePersist();
}

function onPointerUp(event: PointerEvent) {
    dragging.value = false;
    pointerLast = null;
    if (canvas.value?.hasPointerCapture(event.pointerId)) {
        canvas.value.releasePointerCapture(event.pointerId);
    }
}

function onPointerLeave() {
    cursorBlock.inside = false;
}

function resizeCanvas() {
    const element = stage.value;
    if (!element) return;
    viewport.width = element.clientWidth;
    viewport.height = element.clientHeight;
    const dpr = window.devicePixelRatio || 1;
    const next = canvas.value!;
    next.width = Math.max(1, Math.round(viewport.width * dpr));
    next.height = Math.max(1, Math.round(viewport.height * dpr));
    const context = next.getContext('2d');
    context?.setTransform(dpr, 0, 0, dpr, 0, 0);
    redraw();
    scheduleRefresh();
}

// ---------------- 状态持久化（记住用户的选择） ----------------

type PersistedView = { x: number; z: number; scale: number };
type PersistedState = {
    instanceId: string;
    world: string;
    dimension: WorldMapDimensionId;
    views: Partial<Record<WorldMapDimensionId, PersistedView>>;
};

function loadPersisted(): PersistedState | null {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        return raw ? (JSON.parse(raw) as PersistedState) : null;
    } catch {
        return null;
    }
}

function schedulePersist() {
    if (persistTimer !== undefined) window.clearTimeout(persistTimer);
    persistTimer = window.setTimeout(() => {
        persistTimer = undefined;
        const state = loadPersisted() ?? { views: {} };
        state.instanceId = instanceId.value;
        state.world = worldPath.value;
        state.dimension = dimension.value;
        state.views = state.views ?? {};
        state.views[dimension.value] = { x: center.x, z: center.z, scale: scale.value };
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
        } catch {
            /* 存储不可用时静默 */
        }
    }, 300);
}

function restorePersistedView(): boolean {
    const state = loadPersisted();
    const view = state?.views?.[dimension.value];
    if (!view) return false;
    center.x = view.x;
    center.z = view.z;
    scale.value = Math.min(MAX_SCALE, Math.max(MIN_SCALE, view.scale));
    return true;
}

// ---------------- 生命周期 ----------------

onMounted(async () => {
    resizeCanvas();
    resizeObserver = new ResizeObserver(resizeCanvas);
    if (stage.value) resizeObserver.observe(stage.value);
    canvas.value?.addEventListener('wheel', onWheel, { passive: false });
    try {
        await loadInstances();
        const requested = store.worldMapRequest;
        const persisted = loadPersisted();
        if (requested) {
            store.worldMapRequest = null;
            instanceId.value = requested.instanceId;
        } else if (persisted?.instanceId) {
            instanceId.value = persisted.instanceId;
        }
        await loadWorlds();
        const targetWorld = requested?.world ?? persisted?.world;
        if (targetWorld && worlds.value.some((w) => w.path === targetWorld)) {
            worldPath.value = targetWorld;
        }
        if (persisted?.dimension && DIMENSIONS.some((d) => d.id === persisted.dimension)) {
            dimension.value = persisted.dimension;
        }
    } catch (error) {
        infoError.value = error instanceof Error ? error.message : String(error);
    }
});

onBeforeUnmount(() => {
    resizeObserver?.disconnect();
    canvas.value?.removeEventListener('wheel', onWheel);
    if (refreshTimer !== undefined) window.clearTimeout(refreshTimer);
    if (persistTimer !== undefined) window.clearTimeout(persistTimer);
    if (redrawFrame !== undefined) window.cancelAnimationFrame(redrawFrame);
    tiles.clear();
    // 释放画布后备存储，瓦片纹理不再滞留 GPU 进程
    if (canvas.value) {
        canvas.value.width = 0;
        canvas.value.height = 0;
    }
});

watch(instanceId, () => {
    void loadWorlds().then(() => loadInfo());
});
watch(dimension, () => {
    redraw();
    if (!restorePersistedView()) fitView();
    scheduleRefresh();
    schedulePersist();
});

/** 重新扫描：玩完游戏回来刷新 region 清单（保留当前视野） */
function rescan() {
    levelKeyMemo.clear();
    void loadInfo(true);
}

const scaleLabel = computed(() => {
    const s = scale.value;
    if (s >= 1) return `1 像素 = ${s < 10 ? s.toFixed(1) : Math.round(s)} 方块`;
    return `${(1 / s).toFixed(1)} 像素 = 1 方块`;
});
</script>

<template>
    <section class="worldmap-page" data-ui="worldmap:page">
        <header class="page-head wm-heading" data-ui="worldmap:heading">
            <div>
                <h1 data-ui="WorldMapView:8f2a10c4">世界地图</h1>
                <p class="muted">
                    把存档里实际生成过的地形渲染成 2D 地图；与「种子地图」互补，一个看推演、一个看现状。
                </p>
            </div>
            <div class="actions">
                <button
                    class="btn btn-ghost"
                    data-ui="worldmap:fit"
                    :disabled="!dimensionRegions.length"
                    @click="fitView()"
                >
                    重置视图</button
                ><button
                    class="btn btn-ghost"
                    data-ui="worldmap:rescan"
                    :disabled="infoLoading || !worldPath"
                    @click="rescan"
                >
                    {{ infoLoading ? '扫描中…' : '↻ 重新扫描' }}
                </button>
            </div>
        </header>
        <div class="worldmap-context">
            <label class="wm-field">
                <span>游戏实例</span
                ><SelectMenu
                    v-model="instanceId"
                    :options="instanceOptions"
                    :disabled="!instances.length"
                    placeholder="选择游戏实例"
                    aria-label="游戏实例"
                />
            </label>
            <label class="wm-field">
                <span>存档</span
                ><SelectMenu
                    v-model="worldPath"
                    :options="worldOptions"
                    :disabled="!worlds.length"
                    placeholder="选择存档"
                    aria-label="存档"
                />
            </label>
            <div class="wm-field" role="tablist" aria-label="维度">
                <span>维度</span>
                <div class="wm-dims">
                    <button
                        v-for="dim in DIMENSIONS"
                        :key="dim.id"
                        class="btn btn-ghost wm-dim"
                        :class="{ active: dimension === dim.id }"
                        :disabled="dimensionEmpty(dim.id)"
                        :title="
                            dimensionEmpty(dim.id) && info
                                ? '该维度还没有生成过任何区块；进游戏去过一次就会出现'
                                : undefined
                        "
                        @click="dimension = dim.id"
                    >
                        {{ dim.label }}
                    </button>
                </div>
            </div>
        </div>
        <div class="worldmap-library" data-ui="worldmap:library">
            <ResourcePathChip v-if="info" label="存档位置" :path="info.worldPath" />
            <details class="wm-help">
                <summary>使用说明</summary>
                <p>
                    地图数据来自存档的 region 文件（1.13+ 版本格式），按需渲染、磁盘缓存：第一次看某个区域要现算（大世界全貌可能要等几十秒，之后秒开）。玩完游戏点「重新扫描」即可看到新地形。拖拽平移、滚轮缩放；下界视图会自动穿透顶部基岩天花板。渲染是只读操作，不会改动存档。
                </p>
            </details>
        </div>
        <section class="worldmap-stage card" ref="stage" data-ui="worldmap:stage">
            <canvas
                ref="canvas"
                class="worldmap-canvas"
                :class="{ grabbable: !dragging, grabbing: dragging }"
                @pointerdown="onPointerDown"
                @pointermove="onPointerMove"
                @pointerup="onPointerUp"
                @pointercancel="onPointerUp"
                @pointerleave="onPointerLeave"
            ></canvas>

            <div v-if="infoLoading && !info" class="wm-overlay">
                <span class="wm-spinner" aria-hidden="true"></span>
                <p>正在扫描存档…</p>
            </div>

            <div v-else-if="infoError" class="wm-overlay">
                <p class="wm-overlay-title">无法读取这个存档</p>
                <p class="muted">{{ infoError }}</p>
                <button class="btn btn-ghost" @click="rescan">重试</button>
            </div>

            <div v-else-if="!instances.length" class="wm-overlay">
                <p class="wm-overlay-title">还没有可用的游戏实例</p>
                <p class="muted">先在「启动」页安装一个游戏版本，玩过一次生成存档后就能看地图。</p>
            </div>

            <div v-else-if="!worlds.length" class="wm-overlay">
                <p class="wm-overlay-title">这个实例还没有存档</p>
                <p class="muted">启动一次游戏创建世界（或从实例中心导入存档）后，这里就会出现地图。</p>
            </div>

            <div v-else-if="info && !info.supported" class="wm-overlay">
                <p class="wm-overlay-title">暂不支持该存档版本</p>
                <p class="muted">{{ info.unsupportedReason ?? '该存档版本过旧，地图预览暂不支持。' }}</p>
            </div>

            <div v-else-if="info && !dimensionRegions.length" class="wm-overlay">
                <p class="wm-overlay-title">该维度还没有生成区块</p>
                <p class="muted">进游戏到对应维度走过一次再来，或点右上角「重新扫描」。</p>
            </div>

            <footer v-if="info?.supported" class="wm-status">
                <span class="wm-coord" data-ui="worldmap:coord">
                    {{ cursorBlock.inside ? `X ${cursorBlock.x} · Z ${cursorBlock.z}` : 'X – · Z –' }}
                </span>
                <span class="wm-sep" aria-hidden="true">|</span>
                <span>{{ scaleLabel }}</span>
                <span class="wm-grow"></span>
                <span v-if="tileLoading" class="wm-loading"
                    ><span class="wm-spinner" aria-hidden="true"></span>加载瓦片…</span
                >
                <button
                    v-if="tileErrors"
                    class="btn btn-sm"
                    data-ui="worldmap:retry"
                    :title="tileErrorMessage || undefined"
                    @click="retryTiles"
                >
                    {{ tileErrors }} 张瓦片失败，点击重试
                </button>
                <span class="muted">拖拽平移 · 滚轮缩放</span>
            </footer>
        </section>
    </section>
</template>

<style scoped>
/* 页面骨架与录像/截图页同构：grid 容器 + heading 左右布局 + 上下文行 + 工作区卡片 */
.worldmap-page {
    max-width: 1440px;
    margin: 0 auto;
    display: grid;
    gap: 16px;
    min-width: 0;
}
.wm-heading {
    display: flex;
    flex-direction: row;
    text-align: left;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
}
.wm-heading h1 {
    margin: 0 0 8px;
    font-size: 24px;
}
.wm-heading p {
    margin: 4px 0;
}
.actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
}
.worldmap-context {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    flex-wrap: wrap;
}
.wm-field {
    display: grid;
    gap: 6px;
    font-size: 12px;
    color: var(--text-dim);
    min-width: 0;
}
.wm-field :deep(.select-menu-btn) {
    min-width: 220px;
}
.wm-dims {
    display: flex;
    gap: 6px;
}
/* 激活态用成套底色语言（同实例中心 ic-tabs），不只靠一圈描边 */
.wm-dim.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 700;
}
.wm-dim:disabled {
    opacity: 0.45;
}
.worldmap-library {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    flex-wrap: nowrap;
    min-width: 0;
}
/* 使用说明与存档位置并排：开关钉在右侧、整条可点；展开/收起时开关位置不变（控件不得因交互跳动） */
.wm-help {
    margin-left: auto;
    flex: 0 1 auto;
    min-width: 0;
    max-width: min(640px, 55%);
    font-size: 12px;
    color: var(--text-dim);
}
.wm-help summary {
    display: block;
    text-align: right;
    cursor: pointer;
    white-space: nowrap;
}
/* display:block 会吃掉 <summary> 默认三角，这里显式补回展开方向箭头 */
.wm-help summary::after {
    content: " ▸";
}
.wm-help[open] summary::after {
    content: " ▾";
}
.wm-help p {
    margin: 8px 0 0;
    line-height: 1.7;
}
/* 地图画布吃满剩余视口高度（页面本身不滚动，地图内部平移） */
.worldmap-stage {
    position: relative;
    height: calc(100vh - 280px);
    min-height: 420px;
    overflow: hidden;
    display: flex;
    padding: 0;
}
.worldmap-canvas {
    flex: 1;
    width: 100%;
    height: 100%;
    display: block;
    touch-action: none;
}
.worldmap-canvas.grabbable {
    cursor: grab;
}
.worldmap-canvas.grabbing {
    cursor: grabbing;
}
.wm-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    padding: 24px;
    background: color-mix(in srgb, var(--surface-2, #17191d) 55%, transparent);
}
.wm-overlay-title {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
}
.wm-overlay .muted {
    max-width: 460px;
    margin: 0;
}
.wm-status {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 12px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--surface-3, #23262d) 88%, transparent);
    font-size: 12px;
    color: var(--text-dim);
}
.wm-coord {
    font-variant-numeric: tabular-nums;
}
.wm-sep {
    opacity: 0.4;
}
.wm-grow {
    flex: 1;
}
.wm-loading {
    display: inline-flex;
    align-items: center;
    gap: 6px;
}
.wm-spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--text-dim);
    border-top-color: transparent;
    animation: wm-spin 0.9s linear infinite;
    display: inline-block;
}
@keyframes wm-spin {
    to {
        transform: rotate(360deg);
    }
}
</style>
