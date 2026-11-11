import faceUrl from './assets/splash-face.png';
import {
    makeBootTiles,
    advanceBootTiles,
    canAssembleBoot,
    BOOT_FLOAT_MIN_MS,
    CONVERGE_DURATION,
    ASSEMBLED_HOLD_MS,
    BOOT_FRAME_MS,
    type BootState,
    type BootPoint
} from '@shared/startup';
import { drawBootFrame } from './splashPaint';
import './splash.css';

const bridge = window.kamuclSplash;
const canvas = document.querySelector<HTMLCanvasElement>('#glass')!;
const ctx = canvas.getContext('2d', { alpha: true })!;
const caption = document.querySelector<HTMLSpanElement>('#stage')!;
const face = new Image();
let state: BootState = { completed: [], ready: false },
    pointer: BootPoint | null = null;
let geometry = makeBootTiles(innerWidth, innerHeight);
let start = 0,
    convergence: number | null = null,
    raf = 0,
    assembled = false,
    lastFrame = 0;
let nextFrame = 0;
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
const labels = ['读取配置', '加载账户', '扫描游戏实例', '加载首页图片、Java 与皮肤', '准备首帧'];
function resize() {
    const dpr = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(innerWidth * dpr);
    canvas.height = Math.round(innerHeight * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;
    geometry = makeBootTiles(innerWidth, innerHeight);
    // 加载卡贴在拼合板下方：板尺寸随窗口变，写死 top 会在小窗上盖住头像
    caption.style.top = `${geometry.top + geometry.board + 26}px`;
    convergence = null;
}
const offPointer = bridge.onPointer((next) => {
    pointer = next;
});
const offState = bridge.onState((next) => {
    state = next;
    caption.textContent = state.ready
        ? '准备就绪'
        : `${labels[Math.min(4, state.completed.length)]}…`;
});
const offReveal = bridge.onReveal(() => {
    document.body.classList.add('leaving');
    document.body.addEventListener('transitionend', () => bridge.finished(), { once: true });
});
function frame(now: number) {
    if (now + 0.5 < nextFrame) {
        raf = requestAnimationFrame(frame);
        return;
    }
    if (!nextFrame) nextFrame = now;
    nextFrame += BOOT_FRAME_MS;
    if (nextFrame <= now) nextFrame = now + BOOT_FRAME_MS;
    if (!start) start = now;
    const elapsed = now - start,
        delta = lastFrame ? now - lastFrame : BOOT_FRAME_MS;
    lastFrame = now;
    if (canAssembleBoot(state) && convergence === null && (reduced || elapsed >= BOOT_FLOAT_MIN_MS))
        convergence = elapsed;
    if (convergence === null && !reduced) advanceBootTiles(geometry, elapsed, delta, pointer);
    ctx.clearRect(0, 0, innerWidth, innerHeight);
    const complete =
        reduced || (convergence !== null && elapsed - convergence >= CONVERGE_DURATION);
    drawBootFrame(ctx, face, geometry, elapsed, convergence, complete);
    if (
        !assembled &&
        state.ready &&
        convergence !== null &&
        (reduced || elapsed - convergence >= CONVERGE_DURATION + ASSEMBLED_HOLD_MS)
    ) {
        assembled = true;
        raf = requestAnimationFrame(() => bridge.assembled());
    } else if (!assembled) raf = requestAnimationFrame(frame);
}
face.onload = () => {
    document.body.dataset.motion = reduced ? 'system-reduced' : 'full';
    resize();
    raf = requestAnimationFrame(frame);
    bridge.ready();
};
let usingCustomFace = !!window.__KAMUCL_SPLASH_FACE__;
face.onerror = () => {
    // 注入的自定义头像加载失败时回退内置默认图；默认图也失败才报启动失败
    if (usingCustomFace) {
        usingCustomFace = false;
        face.src = faceUrl;
        return;
    }
    bridge.failed('启动头像资源无法加载');
};
face.src = usingCustomFace ? window.__KAMUCL_SPLASH_FACE__! : faceUrl;
window.addEventListener('resize', resize);
window.addEventListener('error', () => bridge.failed('启动动画渲染失败'));
window.addEventListener('unload', () => {
    cancelAnimationFrame(raf);
    offPointer();
    offState();
    offReveal();
    window.removeEventListener('resize', resize);
});
