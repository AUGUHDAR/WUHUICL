import {
    BOOT_GRID,
    tilePose,
    type BootGeometry,
    type BootTile,
    type TilePose
} from '@shared/startup';

/** 启动页头像图：内置 PNG 或后端注入的自定义头像 data URL */
export interface SplashFace {
    readonly width: number;
    readonly height: number;
}

/**
 * 一块玻璃像素碎片：延续 KAMUCL 旧版玻璃碎片的质感（玻璃渐变 + 亮斜边 + 折射细边），
 * 但切片从玻璃三角改成整齐的头像像素方块——散开时是玻璃屑，落位时严丝合缝拼成头像。
 * 玻璃层随 progress 淡尽，像素本身随之升到完全不透明。
 */
export function drawBootTile(
    ctx: CanvasRenderingContext2D,
    face: CanvasImageSource & SplashFace,
    tile: BootTile,
    pose: TilePose
) {
    // 源切片按 BOOT_GRID 等分，末列/末行吃满余数，任何图尺寸都能整齐切完
    const sx = Math.round((face.width * tile.col) / BOOT_GRID),
        sy = Math.round((face.height * tile.row) / BOOT_GRID),
        ex = Math.round((face.width * (tile.col + 1)) / BOOT_GRID),
        ey = Math.round((face.height * (tile.row + 1)) / BOOT_GRID);
    const size = tile.size,
        half = size / 2,
        glass = 1 - pose.progress;
    ctx.save();
    ctx.translate(pose.x, pose.y);
    ctx.rotate(pose.rotation);
    ctx.scale(pose.scale, pose.scale);
    const rect = new Path2D();
    rect.rect(-half, -half, size, size);
    ctx.save();
    ctx.clip(rect);
    // 碎片期就保持头像像素可辨（0.78 起）：散开时每块仍看得出是发丝、皮肤还是眼睛，
    // 而不是一串同样的灰方块；落位时升到完全不透明
    ctx.globalAlpha = 0.78 + 0.22 * pose.progress;
    ctx.drawImage(face, sx, sy, ex - sx, ey - sy, -half, -half, size, size);
    ctx.restore();
    if (glass > 0) {
        ctx.globalAlpha = glass;
        const tint = ctx.createLinearGradient(-half, -half, half, half);
        tint.addColorStop(0, '#effaff55');
        tint.addColorStop(0.4, '#badfff08');
        tint.addColorStop(0.52, '#ffffff30');
        tint.addColorStop(1, '#bcb0fa12');
        ctx.fillStyle = tint;
        ctx.fill(rect);
        // 深色包边给出玻璃厚度，亮细边是折射光；两笔叠加才读出「玻璃」而不是「线框」
        ctx.strokeStyle = '#2533494d';
        ctx.lineWidth = Math.max(1, size / 18);
        ctx.stroke(rect);
        ctx.strokeStyle = '#eaf7ffb0';
        ctx.lineWidth = 0.85;
        ctx.stroke(rect);
        // 一条随姿态呼吸的亮斜边：碎片自己也在转，光一直在动
        ctx.beginPath();
        ctx.moveTo(-half + 0.5, half - 0.5);
        ctx.lineTo(-half + 0.5, -half + 0.5);
        ctx.lineTo(half - 0.5, -half + 0.5);
        ctx.strokeStyle = `rgba(255,255,255,${(0.5 + Math.sin(tile.phase + pose.rotation * 3) * 0.25) * glass})`;
        ctx.lineWidth = 1.2;
        ctx.stroke();
    }
    ctx.restore();
}

/**
 * 画一帧启动页：complete 时整图落位，否则逐块画当前姿态。
 * 时间推进与启动门都在调用方，这里只做绘制，方便离线预览按任意时刻定格。
 */
export function drawBootFrame(
    ctx: CanvasRenderingContext2D,
    face: CanvasImageSource & SplashFace,
    geometry: BootGeometry,
    elapsed: number,
    convergeAt: number | null,
    complete: boolean
) {
    if (complete) {
        ctx.drawImage(face, geometry.left, geometry.top, geometry.board, geometry.board);
        return;
    }
    for (const tile of geometry.tiles)
        drawBootTile(ctx, face, tile, tilePose(tile, elapsed, convergeAt, geometry));
}
