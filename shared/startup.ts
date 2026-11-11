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

export const BOOT_FLOAT_MIN_MS = 900;
export const CONVERGE_DURATION = 1400;
export const ASSEMBLED_HOLD_MS = 2000;
export const BOOT_FRAME_MS = 1000 / 60;
export const POINTER_RADIUS = 140;
/**
 * 头像切成 BOOT_GRID² 块像素碎片。
 * 8×8＝64 块：头像原图正是 8×8 个 32px 的美术像素，于是每块碎片就是头像的一个像素，
 * 散开时每块都有确定的颜色（发丝/皮肤/眼睛各不同），落位后就是原图的马赛克拼合。
 * 网格数必须整除 8：12 会切进美术像素内部，拼合处出现碎边。
 */
export const BOOT_GRID = 8;

export interface BootPoint {
    x: number;
    y: number;
}
export interface TilePose extends BootPoint {
    rotation: number;
    scale: number;
    progress: number;
}
export interface BootTile {
    /** 源图切片索引；取像素矩形时按实际图宽高换算，保证 144 块严丝合缝地拼回整张图 */
    col: number;
    row: number;
    /** 落位后的边长与中心 */
    size: number;
    targetX: number;
    targetY: number;
    /** 散布期的漂移中心（整窗坐标） */
    x: number;
    y: number;
    phase: number;
    spin: number;
    delay: number;
    offsetX: number;
    offsetY: number;
    velocityX: number;
    velocityY: number;
    from?: TilePose;
}
export interface BootGeometry {
    board: number;
    left: number;
    top: number;
    size: number;
    width: number;
    height: number;
    tiles: BootTile[];
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

/**
 * 64 块头像像素碎片铺满整窗漂浮，最后逐块落位拼回头像。
 * 散布范围是全窗（四边留一块碎片的安全边），不是拼合板四周的紧凑带：
 * 碎片散开才是开屏的观感，只有落位瞬间才收拢到中间的拼图板。
 */
export function makeBootTiles(width: number, height: number): BootGeometry {
    let seed = 1041;
    const random = () => {
        seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
        return seed / 4294967296;
    };
    // 板宽取 BOOT_GRID 的整数倍：每块边长整数，落位时相邻块不会出现半像素缝。
    // 上限 280＝每块 35px：64 块散满整窗时仍读得出是「头像碎片」，
    // 但拼合后的头像不至于占满小半屏（384 时每块 48px、整块 384px，观感偏重）。
    const size = Math.max(7, Math.floor(Math.min(280, width * 0.24, height * 0.32) / BOOT_GRID));
    const board = size * BOOT_GRID;
    const left = Math.round((width - board) / 2),
        top = Math.round((height - board) / 2);
    const tiles: BootTile[] = [];
    // 分层抖动网格：把整窗切成 BOOT_GRID² 个格子，每块落进独立格子再格内随机。
    // 纯随机会在中间扎堆、四角留空；分格散布才是「铺满整屏」的观感
    const cellW = width / BOOT_GRID,
        cellH = height / BOOT_GRID;
    for (let row = 0; row < BOOT_GRID; row++)
        for (let col = 0; col < BOOT_GRID; col++) {
            // 旋转后的外接半径（45° 最坏情况）；留这个余量碎片才不会顶出窗口被裁掉
            const margin = size * 0.75 + 6;
            const loX = Math.min(margin, width / 2),
                loY = Math.min(margin, height / 2);
            // 格内留边，避免相邻格子里的碎片各自贴边重叠成「双块」
            const inset = Math.min(cellW, cellH) * 0.16;
            const jx = Math.max(0, cellW - inset * 2),
                jy = Math.max(0, cellH - inset * 2);
            tiles.push({
                col,
                row,
                size,
                targetX: left + (col + 0.5) * size,
                targetY: top + (row + 0.5) * size,
                x: clampTo(col * cellW + inset + random() * jx, loX, width - loX),
                y: clampTo(row * cellH + inset + random() * jy, loY, height - loY),
                phase: random() * Math.PI * 2,
                // 像素块允许小幅转角：散开时要能读出「每块都在自己转」，太大又会读出「方块」
                spin: (random() - 0.5) * 1.4,
                delay: random() * 220,
                offsetX: 0,
                offsetY: 0,
                velocityX: 0,
                velocityY: 0
            });
        }
    return { board, left, top, size, width, height, tiles };
}

/** 漂浮期只做缓速漂移，幅度随窗口等比（大屏不显得静止、小窗不显得乱窜）。 */
const driftScale = (width: number, height: number) =>
    Math.max(0.6, Math.min(1.6, Math.min(width, height) / 800));

const tileBase = (tile: BootTile, elapsed: number, width: number, height: number): BootPoint => {
    const s = driftScale(width, height);
    return {
        x: tile.x + Math.sin(elapsed / 1500 + tile.phase) * 16 * s,
        y: tile.y + Math.cos(elapsed / 1800 + tile.phase) * 13 * s
    };
};

function floating(tile: BootTile, elapsed: number, width: number, height: number): TilePose {
    const base = tileBase(tile, elapsed, width, height);
    return {
        x: base.x + tile.offsetX,
        y: base.y + tile.offsetY,
        rotation: tile.spin + Math.sin(elapsed / 2100 + tile.phase) * 0.16,
        scale: 1 + Math.sin(elapsed / 1900 + tile.phase) * 0.08,
        progress: 0
    };
}

/** 有界弹簧：光标靠近时把碎片推开，但碎片永远留在窗口内。 */
export function advanceBootTiles(
    geometry: BootGeometry,
    elapsed: number,
    deltaMs: number,
    pointer: BootPoint | null
): void {
    const dt = Math.min(32, Math.max(0, deltaMs)) / 1000;
    const { width, height } = geometry;
    for (const tile of geometry.tiles) {
        const p = floating(tile, elapsed, width, height),
            m = tile.size * 0.75 + 6;
        let fx = 0,
            fy = 0;
        // 贴窗口边会被裁掉半块，留一段缓冲带把碎片推回窗内
        const band = m + 40;
        if (p.x < band) fx += (band - p.x) * 42;
        if (p.x > width - band) fx -= (p.x - (width - band)) * 42;
        if (p.y < band) fy += (band - p.y) * 42;
        if (p.y > height - band) fy -= (p.y - (height - band)) * 42;
        if (pointer) {
            let dx = p.x - pointer.x,
                dy = p.y - pointer.y;
            const distance = Math.hypot(dx, dy);
            if (distance < POINTER_RADIUS) {
                if (distance < 1) {
                    dx = Math.cos(tile.phase);
                    dy = Math.sin(tile.phase);
                }
                const force = (2400 * (1 - distance / POINTER_RADIUS) ** 2) / Math.max(1, distance);
                fx = dx * force;
                fy = dy * force;
            }
        }
        const drag = Math.exp(-5 * dt);
        tile.velocityX = (tile.velocityX + (fx - tile.offsetX * 8) * dt) * drag;
        tile.velocityY = (tile.velocityY + (fy - tile.offsetY * 8) * dt) * drag;
        // 弹簧位移限幅小于缓冲带，保证硬约束永远有解
        tile.offsetX = Math.max(-180, Math.min(180, tile.offsetX + tile.velocityX * dt));
        tile.offsetY = Math.max(-180, Math.min(180, tile.offsetY + tile.velocityY * dt));
        // 硬约束兜底：位移限幅之上再夹一次，碎片绝不越过窗口边
        const base = tileBase(tile, elapsed, width, height);
        if (base.x + tile.offsetX < m) {
            tile.offsetX = m - base.x;
            tile.velocityX = Math.max(0, tile.velocityX);
        } else if (base.x + tile.offsetX > width - m) {
            tile.offsetX = width - m - base.x;
            tile.velocityX = Math.min(0, tile.velocityX);
        }
        if (base.y + tile.offsetY < m) {
            tile.offsetY = m - base.y;
            tile.velocityY = Math.max(0, tile.velocityY);
        } else if (base.y + tile.offsetY > height - m) {
            tile.offsetY = height - m - base.y;
            tile.velocityY = Math.min(0, tile.velocityY);
        }
    }
}

/**
 * 落位时冻结漂浮姿态再插值，光标在附近也不会跳变。
 * 落位终点就是拼图格里那一小块的位置，144 块叠加即整张头像。
 */
export function tilePose(
    tile: BootTile,
    elapsed: number,
    convergeAt: number | null,
    geometry: BootGeometry
): TilePose {
    if (convergeAt === null) {
        const pose = floating(tile, elapsed, geometry.width, geometry.height),
            m = tile.size * 0.75 + 6;
        // 物理约束之外每帧再夹一次：碎片始终完整落在窗内
        pose.x = clampTo(pose.x, m, geometry.width - m);
        pose.y = clampTo(pose.y, m, geometry.height - m);
        return pose;
    }
    tile.from ??= floating(tile, convergeAt, geometry.width, geometry.height);
    const progress = smooth((elapsed - convergeAt - tile.delay) / (CONVERGE_DURATION - 220)),
        from = tile.from;
    if (progress === 1)
        return { x: tile.targetX, y: tile.targetY, rotation: 0, scale: 1, progress: 1 };
    return {
        x: from.x + (tile.targetX - from.x) * progress,
        y: from.y + (tile.targetY - from.y) * progress,
        rotation: from.rotation * (1 - progress),
        scale: from.scale + (1 - from.scale) * progress,
        progress
    };
}
