<script setup lang="ts">
// 首页启动卡像素动画画布：以可调节节拍（默认 40fps）向后端请求无状态渲染帧。
// 帧是网格分辨率的原始像素（base64 u32），写入 ImageData 后整体缩放到画布
// （imageSmoothing 关闭保持锐度）——暗角/星空的抖动会把矩形协议压碎成上万
// 碎矩形，逐矩形绘制撑不住 40fps，这是当初掉卡的根因。
// 时钟/轮播/过渡（区块加载式）都在前端持有；悬停整卡、页面隐藏、reduceMotion
// 时退化为静止帧，语义对齐 LumilioCL hero 的 Director。
// 场景渲染：src-tauri/src/hero_scenes/（vendor，AGPL-3.0；起飞为 WUHUICL 原创）。
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useMotion } from '../motion';
import {
    HERO_SCENES,
    HERO_TICK_MS,
    heroGridLayout,
    heroSceneId,
    heroStillAge,
    loadHeroBookmark,
    requestHeroFrame,
    saveHeroBookmark
} from '../heroScenes';

const props = withDefaults(
    defineProps<{
        /** 悬停整卡时暂停（时钟冻结，画面静止）。 */
        hoverPaused?: boolean;
        /** 指针悬停开始游戏：炉火/起飞回应（flare 缓升缓降）。 */
        flareTarget?: boolean;
        /** 启动进度联动：active 时世界按 progress 逐块加载，轮播暂停。 */
        loading?: { active: boolean; progress: number };
        /** 游戏运行中：场景压暗为静止帧。 */
        dimmed?: boolean;
        /** 当前实例标识（炉火地标来源）。 */
        instanceKey?: string;
        /** 紧凑卡（Android 舞台）：隐藏指示条。 */
        compact?: boolean;
        /** 动画速度倍率（作用在场景时钟上；停留按场景时间计）。 */
        speed?: number;
        /** 每场景停留时长（场景秒）。 */
        dwellSeconds?: number;
    }>(),
    {
        hoverPaused: false,
        flareTarget: false,
        loading: () => ({ active: false, progress: 0 }),
        dimmed: false,
        instanceKey: '',
        compact: false,
        speed: 1,
        dwellSeconds: 9.5
    }
);
const emit = defineEmits<{ fallback: [reason: string]; scene: [entry: (typeof HERO_SCENES)[number]] }>();

const { decorativeActive } = useMotion();
const canvasRef = ref<HTMLCanvasElement | null>(null);
const rootRef = ref<HTMLDivElement | null>(null);
const hud = ref<string | null>(null);
const current = ref(loadHeroBookmark());
const dwell = ref(0);
onMounted(() => emit('scene', HERO_SCENES[current.value]));
watch(current, (index) => emit('scene', HERO_SCENES[index]));

// ---------------- 时钟状态（非响应式：每 tick 就地读写） ----------------
let columns = 0;
let rows = 0;
let inflight: Promise<unknown> | null = null;
let failures = 0;
let fellBack = false;
let raf = 0;
let lastTickAt = 0;
let lastFrameAt = 0;
let sceneStartedAt = 0; // performance.now()，当前场景 0 时刻
let leaving: { index: number; startedAt: number; ageAtHandoff: number } | null = null;
let flare = 0;
/** 网格分辨率的离屏画布与 ImageData（尺寸变化时重建）。 */
let stageCanvas: HTMLCanvasElement | null = null;
let stageImage: ImageData | null = null;
const still = () => !decorativeActive.value || props.dimmed;
/** 场景时钟（秒）：速度倍率作用在这里，停留与画面推进共用同一时间轴。 */
const sceneAge = (now: number) => Math.max(0, (now - sceneStartedAt) / 1000) * props.speed;

const loading = computed(() => props.loading.active);
const scenes = computed(() =>
    HERO_SCENES.map((entry, index) => ({
        ...entry,
        index,
        active: index === current.value,
        title: entry.note ? `${entry.label} · ${entry.note}` : entry.label
    }))
);

function select(index: number) {
    if (still()) {
        current.value = index;
        saveHeroBookmark(index);
        renderStillFrame();
        return;
    }
    if (index === current.value && !leaving) {
        sceneStartedAt = performance.now();
        return;
    }
    startTransition(index);
}

function startTransition(toIndex: number) {
    const now = performance.now();
    leaving = {
        index: current.value,
        startedAt: now,
        // 离场场景时钟冻结在交接时刻：后端可整帧复用，转场开销减半；
        // 涌入的新场景会逐块盖住它，静止的旧画面无违和。
        ageAtHandoff: sceneAge(now)
    };
    current.value = toIndex;
    sceneStartedAt = now;
    dwell.value = 0;
    lastFrameAt = now; // 下一拍按正常节发出，避免手动切换瞬间每 rAF 连发
    saveHeroBookmark(toIndex);
}

// ---------------- 帧请求与绘制 ----------------

function frameRequest(now: number) {
    const request: Parameters<typeof requestHeroFrame>[0] = {
        scene: heroSceneId(HERO_SCENES[current.value].id, props.instanceKey),
        age: sceneAge(now),
        columns,
        rows
    };
    if (leaving) {
        request.leaving = {
            scene: heroSceneId(HERO_SCENES[leaving.index].id, props.instanceKey),
            age: leaving.ageAtHandoff,
            progress: Math.min(1, (now - leaving.startedAt) / 1000 / 1.2)
        };
    }
    if (loading.value) request.reveal = Math.min(1, Math.max(0, props.loading.progress));
    if (props.dimmed) request.dim = true;
    if (flare > 0.005) request.flare = Math.min(1, flare);
    return request;
}

/** base64 u32 像素流 → 逐 texel 颜色数组（0xRRGGBB，行主序）。 */
function decodePixels(bin: string): Uint32Array {
    const raw = atob(bin);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    return new Uint32Array(bytes.buffer, 0, raw.length / 4);
}

function ensureStage(): ImageData | null {
    if (!columns || !rows) return null;
    if (!stageCanvas || !stageImage || stageImage.width !== columns || stageImage.height !== rows) {
        stageCanvas = document.createElement('canvas');
        stageCanvas.width = columns;
        stageCanvas.height = rows;
        const stageCtx = stageCanvas.getContext('2d');
        if (!stageCtx) return null;
        stageImage = stageCtx.createImageData(columns, rows);
    }
    return stageImage;
}

/** 逐 texel 写入 ImageData 后整体缩放：一次 drawImage 替代上万次 fillRect。 */
function paint(pixels: Uint32Array) {
    const canvas = canvasRef.value;
    const image = ensureStage();
    if (!canvas || !image || !stageCanvas) return;
    const view = new Uint32Array(image.data.buffer);
    for (let i = 0; i < view.length && i < pixels.length; i++) {
        // 帧像素是 0xRRGGBB；ImageData 的字节序是 RGBA，小端 u32 视角为
        // 0xAABBGGRR——必须把 R/B 交换后再写，直接写 0xFFRRGGBB 会红蓝互换。
        const color = pixels[i];
        view[i] = (0xff000000 | ((color & 0xff) << 16)) | (color & 0xff00) | (color >>> 16);
    }
    const stageCtx = stageCanvas.getContext('2d');
    const ctx = canvas.getContext('2d');
    if (!stageCtx || !ctx) return;
    stageCtx.putImageData(image, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(stageCanvas, 0, 0, canvas.width, canvas.height);
}

function renderStillFrame() {
    const canvas = canvasRef.value;
    if (!canvas || !columns || fellBack) return;
    const sceneId = HERO_SCENES[current.value].id;
    requestHeroFrame({
        scene: heroSceneId(sceneId, props.instanceKey),
        age: heroStillAge(sceneId),
        columns,
        rows,
        dim: props.dimmed
    })
        .then((response) => {
            hud.value = response.hud;
            paint(decodePixels(response.bin));
        })
        .catch(() => {
            /* 静止帧失败不打扰；活动帧路径会再计数 */
        });
}

function requestFrame(now: number) {
    if (!columns || inflight) return;
    const request = frameRequest(now);
    inflight = requestHeroFrame(request)
        .then((response) => {
            failures = 0;
            hud.value = loading.value ? `区块加载 ${Math.round((props.loading.progress || 0) * 100)}%` : response.hud;
            paint(decodePixels(response.bin));
        })
        .catch((error) => {
            failures += 1;
            if (failures >= 3 && !fellBack) {
                fellBack = true;
                emit('fallback', String(error));
            }
        })
        .finally(() => {
            inflight = null;
        });
}

// ---------------- 调度 ----------------

function schedule() {
    if (raf || fellBack) return;
    raf = requestAnimationFrame(tick);
}

function tick(now: number) {
    raf = 0;
    if (fellBack) return;
    if (still() || props.hoverPaused || document.hidden) {
        lastTickAt = now;
        schedule();
        return;
    }
    const step = lastTickAt ? (now - lastTickAt) / 1000 : 0;
    lastTickAt = now;
    // flare 缓升缓降（上游 FLARE_RISE 7 / FLARE_FALL 3）。
    const target = props.flareTarget ? 1 : 0;
    const rate = target > flare ? 7 : 3;
    flare = Math.min(1, Math.max(0, flare + Math.sign(target - flare) * rate * step));
    if (leaving && (now - leaving.startedAt) / 1000 >= 1.2) leaving = null;
    if (!loading.value && !leaving) {
        // 停留进度按场景时间推进；步进 2% 才写响应式值，避免 60fps 空转重渲染。
        const progress = Math.min(1, sceneAge(now) / props.dwellSeconds);
        if (progress - dwell.value >= 0.02 || progress >= 1) dwell.value = progress;
        if (dwell.value >= 1) {
            startTransition((current.value + 1) % HERO_SCENES.length);
        }
    }
    if (now - lastFrameAt >= HERO_TICK_MS) {
        lastFrameAt = now;
        void requestFrame(now);
    }
    schedule();
}

// ---------------- 尺寸 ----------------

function resize() {
    const canvas = canvasRef.value;
    const root = rootRef.value;
    if (!canvas || !root) return;
    const dpr = Math.max(1, window.devicePixelRatio || 1);
    const width = Math.round(root.clientWidth * dpr);
    const height = Math.round(root.clientHeight * dpr);
    if (width < 8 || height < 8 || (canvas.width === width && canvas.height === height)) return;
    canvas.width = width;
    canvas.height = height;
    const layout = heroGridLayout(width, height);
    if (layout) {
        columns = layout.columns;
        rows = layout.rows;
        stageCanvas = null;
        stageImage = null;
        if (still()) renderStillFrame();
    }
}

let observer: ResizeObserver | null = null;

onMounted(() => {
    sceneStartedAt = performance.now();
    resize();
    observer = new ResizeObserver(resize);
    if (rootRef.value) observer.observe(rootRef.value);
    schedule();
});

onBeforeUnmount(() => {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    observer?.disconnect();
    saveHeroBookmark(current.value);
});

// 环境恢复（取消 reduceMotion / 游戏退出 / 取消暂停）时补一帧静止画面。
// 页面隐藏由 useMotion 的 decorativeActive 覆盖（其内部监听 visibilitychange）。
watch(
    () => [still(), props.hoverPaused, props.dimmed] as const,
    () => {
        if (still()) renderStillFrame();
    }
);
// 实例切换时炉火地标随之变化：重置场景时钟让新地标从头演绎。
watch(
    () => props.instanceKey,
    () => {
        if (HERO_SCENES[current.value].id === 'hearth') {
            sceneStartedAt = performance.now();
            dwell.value = 0;
        }
    }
);
</script>

<template>
    <div ref="rootRef" class="hero-scene" aria-hidden="true">
        <canvas ref="canvasRef" class="hero-scene-canvas"></canvas>
        <span v-if="!compact && hud" class="hero-scene-hud">{{ hud }}</span>
        <div v-if="!compact && !loading && !dimmed" class="hero-scene-dots" role="tablist" aria-label="切换动画场景">
            <button
                v-for="scene in scenes"
                :key="scene.id"
                type="button"
                role="tab"
                class="hero-scene-dot"
                :class="{ active: scene.active }"
                :aria-selected="scene.active"
                :title="scene.title"
                :aria-label="scene.title"
                @click="select(scene.index)"
            >
                <i class="hero-scene-dot-bar">
                    <b v-if="scene.active" class="hero-scene-dot-fill" :style="{ width: dwell * 100 + '%' }"></b>
                </i>
            </button>
        </div>
    </div>
</template>

<style scoped>
.hero-scene {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
}
.hero-scene-canvas {
    display: block;
    width: 100%;
    height: 100%;
}
/* 指示条是卡片上唯一的交互件；其余区域把鼠标事件留给卡片内容。 */
.hero-scene-dots {
    position: absolute;
    top: 52px;
    right: 44px;
    display: flex;
    gap: 2px;
    pointer-events: auto;
}
.hero-scene-dot {
    padding: 9px 3px;
    border: 0;
    background: none;
    cursor: pointer;
    opacity: 0.85;
}
.hero-scene-dot:hover {
    opacity: 1;
}
.hero-scene-dot-bar {
    display: block;
    width: 10px;
    height: 6px;
    border-radius: 1px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.4);
}
.hero-scene-dot.active .hero-scene-dot-bar {
    width: 30px;
    background: rgba(255, 255, 255, 0.28);
}
.hero-scene-dot-fill {
    display: block;
    height: 100%;
    background: rgba(255, 255, 255, 0.92);
}
.hero-scene-hud {
    position: absolute;
    top: 14px;
    right: 44px;
    padding: 3px 8px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.28);
    color: rgba(255, 255, 255, 0.62);
    font-family: ui-monospace, SFMono-Regular, Consolas, 'Courier New', monospace;
    font-size: 12px;
    line-height: 1.4;
    white-space: nowrap;
}
</style>
