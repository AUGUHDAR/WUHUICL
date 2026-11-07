export const BOOT_STAGES = ['settings', 'accounts', 'instances', 'assets', 'paint'] as const;
export type BootStage = (typeof BOOT_STAGES)[number];
export interface BootState {
    completed: BootStage[];
    ready: boolean;
}

/** Animation never substitutes for actual renderer/compositor readiness. */
export class StartupGate {
    readonly completed = new Set<BootStage>();
    painted = false;
    rendererReady = false;
    assembled = false;
    get state(): BootState {
        return { completed: [...this.completed], ready: this.painted && this.rendererReady };
    }
    get canReveal(): boolean {
        return this.state.ready && this.assembled;
    }
}

export const GLASS_FLOAT_MIN_MS = 900;
export const CONVERGE_DURATION = 1400;
export const ASSEMBLED_HOLD_MS = 2000;
export const BOOT_FRAME_MS = 1000 / 60;
export const GLASS_POINTER_RADIUS = 140;
export interface GlassPoint {
    x: number;
    y: number;
}
export interface GlassPose extends GlassPoint {
    rotation: number;
    scale: number;
    progress: number;
}
export interface GlassShard {
    vertices: GlassPoint[];
    /** Max centroid-to-vertex distance; invariant under rotation, bounds any pose. */
    radius: number;
    sourceX: number;
    sourceY: number;
    targetX: number;
    targetY: number;
    x: number;
    y: number;
    phase: number;
    spin: number;
    delay: number;
    offsetX: number;
    offsetY: number;
    velocityX: number;
    velocityY: number;
    from?: GlassPose;
}
export interface GlassZone {
    /** 漂浮区中心与半宽/半高：碎片只在头像拼合板四周的紧凑中央区活动，不靠近窗口边界 */
    cx: number;
    cy: number;
    hx: number;
    hy: number;
}
export interface GlassGeometry {
    board: number;
    left: number;
    top: number;
    width: number;
    height: number;
    zone: GlassZone;
    shards: GlassShard[];
}
const clamp = (n: number) => Math.max(0, Math.min(1, n));
/** Falls back to the midpoint when a tiny window cannot fit the margin on both sides. */
const clampTo = (v: number, lo: number, hi: number) =>
    lo < hi ? Math.max(lo, Math.min(hi, v)) : (lo + hi) / 2;
const smooth = (n: number) => {
    const t = clamp(n);
    return t * t * t * (t * (t * 6 - 15) + 10);
};

export function canAssembleBoot(state: BootState): boolean {
    return (
        state.ready ||
        ['settings', 'accounts', 'instances', 'assets'].every((stage) =>
            state.completed.includes(stage as BootStage)
        )
    );
}

/** Jittered triangulation tiles the exact portrait; no rectangular pixel tiles. */
export function makeBootGlass(width: number, height: number): GlassGeometry {
    let seed = 1041;
    const random = () => {
        seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
        return seed / 4294967296;
    };
    const board = Math.max(80, Math.floor(Math.min(224, width * 0.26, height * 0.32)));
    const left = Math.round((width - board) / 2),
        top = Math.round((height - board) / 2),
        cell = board / 6;
    // 漂浮区＝拼合板四周的紧凑带（半宽 = 板半宽 + 240：容纳指针斥力的 ±180 弹簧上限 + 半径余量），
    // 小窗口时收缩进窗口并留 40px 离边余量。碎片散满全窗时，贴边碎片叠上透出的桌面会读成
    // 「窗口边界」；理想态是只有中间的碎片聚成头像，屏幕其余部分保持干净的桌面。
    const zone: GlassZone = {
        cx: left + board / 2,
        cy: top + board / 2,
        hx: Math.max(0, Math.min(board / 2 + 240, width / 2 - 40)),
        hy: Math.max(0, Math.min(board / 2 + 240, height / 2 - 40))
    };
    const points: GlassPoint[] = [];
    for (let row = 0; row <= 6; row++)
        for (let col = 0; col <= 6; col++)
            points.push({
                x: (col + (col > 0 && col < 6 ? (random() - 0.5) * 0.65 : 0)) * cell,
                y: (row + (row > 0 && row < 6 ? (random() - 0.5) * 0.65 : 0)) * cell
            });
    const shards: GlassShard[] = [];
    const add = (vertices: GlassPoint[]) => {
        const sourceX = vertices.reduce((n, p) => n + p.x, 0) / 3,
            sourceY = vertices.reduce((n, p) => n + p.y, 0) / 3;
        const relative = vertices.map((p) => ({ x: p.x - sourceX, y: p.y - sourceY }));
        const radius = Math.max(...relative.map((p) => Math.hypot(p.x, p.y)));
        shards.push({
            vertices: relative,
            radius,
            sourceX,
            sourceY,
            targetX: left + sourceX,
            targetY: top + sourceY,
            x: zone.cx + (random() * 2 - 1) * Math.max(0, zone.hx - radius * 1.2 - 24),
            y: zone.cy + (random() * 2 - 1) * Math.max(0, zone.hy - radius * 1.2 - 24),
            phase: random() * Math.PI * 2,
            spin: (random() - 0.5) * 2.4,
            delay: random() * 180,
            offsetX: 0,
            offsetY: 0,
            velocityX: 0,
            velocityY: 0
        });
    };
    for (let row = 0; row < 6; row++)
        for (let col = 0; col < 6; col++) {
            const a = points[row * 7 + col],
                b = points[row * 7 + col + 1],
                c = points[(row + 1) * 7 + col],
                d = points[(row + 1) * 7 + col + 1];
            if ((row + col) % 2) {
                add([a, b, c]);
                add([b, d, c]);
            } else {
                add([a, b, d]);
                add([a, d, c]);
            }
        }
    return { board, left, top, width, height, zone, shards };
}

const glassBase = (shard: GlassShard, elapsed: number): GlassPoint => ({
    x: shard.x + Math.sin(elapsed / 1500 + shard.phase) * 14,
    y: shard.y + Math.cos(elapsed / 1800 + shard.phase) * 11
});

/** Half-extent of any rotated pose (scale ≤1.2) plus wobble headroom and a calm strip off the zone border. */
const glassEdgeMargin = (shard: GlassShard): number => shard.radius * 1.2 + 22;

function floating(shard: GlassShard, elapsed: number): GlassPose {
    const base = glassBase(shard, elapsed);
    return {
        x: base.x + shard.offsetX,
        y: base.y + shard.offsetY,
        rotation: shard.spin + Math.sin(elapsed / 2100 + shard.phase) * 0.22,
        scale: 1.06 + Math.sin(elapsed / 1900 + shard.phase) * 0.12,
        progress: 0
    };
}

/** Bounded spring motion. Cursor sampling is independent of click-through windows. */
export function advanceBootGlass(
    shards: GlassShard[],
    elapsed: number,
    deltaMs: number,
    pointer: GlassPoint | null,
    zone: GlassZone
): void {
    const dt = Math.min(32, Math.max(0, deltaMs)) / 1000;
    const loX = zone.cx - zone.hx,
        hiX = zone.cx + zone.hx,
        loY = zone.cy - zone.hy,
        hiY = zone.cy + zone.hy;
    for (const shard of shards) {
        const p = floating(shard, elapsed),
            m = glassEdgeMargin(shard);
        let fx = 0,
            fy = 0;
        // 碎片只待在中央漂浮区：贴区边和贴窗口边一样会读成「边界」，留 m+64 缓冲带回推
        const band = m + 64;
        if (p.x < loX + band) fx += (loX + band - p.x) * 42;
        if (p.x > hiX - band) fx -= (p.x - (hiX - band)) * 42;
        if (p.y < loY + band) fy += (loY + band - p.y) * 42;
        if (p.y > hiY - band) fy -= (p.y - (hiY - band)) * 42;
        if (pointer) {
            let dx = p.x - pointer.x,
                dy = p.y - pointer.y;
            const distance = Math.hypot(dx, dy);
            if (distance < GLASS_POINTER_RADIUS) {
                if (distance < 1) {
                    dx = Math.cos(shard.phase);
                    dy = Math.sin(shard.phase);
                }
                const force =
                    (2400 * (1 - distance / GLASS_POINTER_RADIUS) ** 2) / Math.max(1, distance);
                fx = dx * force;
                fy = dy * force;
            }
        }
        const drag = Math.exp(-5 * dt);
        shard.velocityX = (shard.velocityX + (fx - shard.offsetX * 8) * dt) * drag;
        shard.velocityY = (shard.velocityY + (fy - shard.offsetY * 8) * dt) * drag;
        shard.offsetX = Math.max(-180, Math.min(180, shard.offsetX + shard.velocityX * dt));
        shard.offsetY = Math.max(-180, Math.min(180, shard.offsetY + shard.velocityY * dt));
        // Hard containment on top of the ±180 spring bound: a shard never leaves the float zone.
        const base = glassBase(shard, elapsed);
        if (base.x + shard.offsetX < loX + m) {
            shard.offsetX = loX + m - base.x;
            shard.velocityX = Math.max(0, shard.velocityX);
        } else if (base.x + shard.offsetX > hiX - m) {
            shard.offsetX = hiX - m - base.x;
            shard.velocityX = Math.min(0, shard.velocityX);
        }
        if (base.y + shard.offsetY < loY + m) {
            shard.offsetY = loY + m - base.y;
            shard.velocityY = Math.max(0, shard.velocityY);
        } else if (base.y + shard.offsetY > hiY - m) {
            shard.offsetY = hiY - m - base.y;
            shard.velocityY = Math.min(0, shard.velocityY);
        }
    }
}

/** Snapshot the displaced pose so assembly cannot jump when the pointer is nearby. */
export function glassPosition(
    shard: GlassShard,
    elapsed: number,
    convergeAt: number | null,
    zone: GlassZone
): GlassPose {
    if (convergeAt === null) {
        const pose = floating(shard, elapsed),
            m = glassEdgeMargin(shard);
        // Per-frame safety net under the physics containment: shards stay in the float zone.
        pose.x = clampTo(pose.x, zone.cx - zone.hx + m, zone.cx + zone.hx - m);
        pose.y = clampTo(pose.y, zone.cy - zone.hy + m, zone.cy + zone.hy - m);
        return pose;
    }
    shard.from ??= floating(shard, convergeAt);
    const progress = smooth((elapsed - convergeAt - shard.delay) / (CONVERGE_DURATION - 180)),
        from = shard.from;
    if (progress === 1)
        return { x: shard.targetX, y: shard.targetY, rotation: 0, scale: 1, progress: 1 };
    return {
        x: from.x + (shard.targetX - from.x) * progress,
        y: from.y + (shard.targetY - from.y) * progress,
        rotation: from.rotation * (1 - progress),
        scale: from.scale + (1 - from.scale) * progress,
        progress
    };
}
