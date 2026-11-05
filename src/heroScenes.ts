// 首页启动卡像素动画：场景注册表 + 帧请求客户端 + 时钟常量。
// 场景渲染在后端（src-tauri/src/hero_scenes/，vendor 自 EdwinZhanCN/LumilioCL
// v0.2.3，AGPL-3.0；takeoff 为 WUHUICL 原创）。本文件只持有时钟与网格布局，
// 每 tick 请求一帧无状态渲染，时钟语义对齐上游 hero/timeline.rs。

import { heroFrame as invokeHeroFrame } from './api';
import type { HeroFrameRequest, HeroFrameResponse } from '@shared/types';

/** 每场景停留时长（场景秒；上游 DWELL_SECONDS，设置 launchThumbnail.animation.dwellSeconds 可调）。 */
export const HERO_DWELL_SECONDS = 9.5;
/** 场景间"区块加载"过渡时长（上游 TRANSITION_SECONDS）。 */
export const HERO_TRANSITION_SECONDS = 1.2;
/** 帧节拍：25ms ≈ 40fps。场景本体按连续时钟渲染，节拍只限制 IPC 频率。 */
export const HERO_TICK_MS = 25;
/** 目标 texel 尺寸（逻辑像素）与网格列数护栏（上游 hero_carousel.rs）。 */
const TARGET_TEXEL = 7;
const MIN_COLUMNS = 32;
const MAX_COLUMNS = 190;

export interface HeroSceneEntry {
    /** 注册表 id（书签/指示条定位用）；请求后端时的实际 id 由 sceneId() 解析。 */
    id: string;
    label: string;
    /** 场景眉标（上游 i18n 的 eyebrow 中文化）。 */
    eyebrow: string;
    /** 场景标题与注释（上游 i18n 的 title/caption 中文化）。 */
    title: string;
    caption: string;
    /** 场景强调色（信息行色点，取自上游 accent）。 */
    accent: string;
    note?: string;
}

/** 轮播顺序与后端 scenes::Scene::ALL 一致。 */
export const HERO_SCENES: HeroSceneEntry[] = [
    {
        id: 'dawn',
        label: '黎明',
        eyebrow: '主世界',
        title: '太阳照常升起，世界照常是方的',
        caption: '新的一天，从第一块方块开始。',
        accent: '#ffc35c'
    },
    {
        id: 'caves',
        label: '洞穴',
        eyebrow: '洞穴',
        title: '火把往前一格，黑暗就退后一格',
        caption: '挖开最后一格石头，岩浆的光自己涌了进来。',
        accent: '#ffb03a'
    },
    {
        id: 'redstone',
        label: '红石',
        eyebrow: '红石',
        title: '信号走上十五格，就悄悄熄灭了',
        caption: '同样长的两条线，只有经过中继器的那条，把灯点亮。',
        accent: '#ff3e1e'
    },
    {
        id: 'portal',
        label: '传送门',
        eyebrow: '下界',
        title: '四乘五格黑曜石，围出一扇门',
        caption: '打火石一敲，紫色的光便漫了出来。',
        accent: '#9a55f5'
    },
    {
        id: 'hearth',
        label: '炉火',
        eyebrow: '营地',
        title: '营火还亮着，故事还没讲完',
        caption: '你离开的那一刻，世界停在了原地。',
        accent: '#ff9a3a',
        note: '地标随当前实例'
    },
    {
        id: 'takeoff',
        label: '起飞',
        eyebrow: '起飞',
        title: '夜色正好，一起去更高的地方',
        caption: '倒计时归零，尾焰替我们照亮了江面。',
        accent: '#62c8e8'
    }
];

/** reduceMotion 静止帧时刻（后端 Scene::still_age 的镜像）。 */
const STILL_AGES: Record<string, number> = {
    dawn: 7.2,
    caves: 9,
    redstone: 3.4,
    portal: 4.6,
    hearth: 3,
    takeoff: 10.2
};
export const heroStillAge = (id: string) => STILL_AGES[id] ?? 0;

const HEARTH_LANDMARKS = ['village', 'portal', 'mine', 'lamp'] as const;

/**
 * 炉火场景 id：地标由实例标识哈希决定（上游 live/library.rs 的
 * seed_of：FNV-1a / 7 % 4），同一实例每次相同。
 */
export function hearthSceneId(instanceKey: string): string {
    let hash = 0x811c9dc5;
    for (let i = 0; i < instanceKey.length; i++) {
        hash ^= instanceKey.charCodeAt(i);
        hash = Math.imul(hash, 0x01000193);
    }
    const landmark = HEARTH_LANDMARKS[Math.floor((hash >>> 0) / 7) % 4];
    return `hearth:${landmark}`;
}

/** 注册表 id → 后端场景 id（炉火解析地标，其余原样）。 */
export function heroSceneId(id: string, instanceKey: string): string {
    return id === 'hearth' ? hearthSceneId(instanceKey || 'default') : id;
}

/** canvas 像素尺寸 → 网格布局（列/行），镜像后端护栏；返回 null 表示尺寸未就绪。 */
export function heroGridLayout(widthPx: number, heightPx: number): { columns: number; rows: number } | null {
    if (widthPx < 8 || heightPx < 8) return null;
    const columns = Math.min(MAX_COLUMNS, Math.max(MIN_COLUMNS, Math.round(widthPx / TARGET_TEXEL)));
    const texel = widthPx / columns;
    const rows = Math.max(12, Math.round(heightPx / texel));
    return { columns, rows };
}

/** 单帧请求（HeroSceneCanvas 每 tick 调用；失败由调用方计数处理）。 */
export const requestHeroFrame = (request: HeroFrameRequest): Promise<HeroFrameResponse> =>
    invokeHeroFrame(request);

// ---------------- 轮播时钟 ----------------

/** 位置书签：记住上次看到的场景，重启后从该场景续播（对齐图片轮播的书签习惯）。 */
const BOOKMARK_KEY = 'kamucl.heroScene';

export function loadHeroBookmark(): number {
    try {
        const saved = JSON.parse(localStorage.getItem(BOOKMARK_KEY) ?? 'null');
        if (typeof saved?.index === 'number' && saved.index >= 0 && saved.index < HERO_SCENES.length) {
            return Math.floor(saved.index);
        }
    } catch {
        /* invalid bookmark */
    }
    return 0;
}

export function saveHeroBookmark(index: number): void {
    try {
        localStorage.setItem(BOOKMARK_KEY, JSON.stringify({ index }));
    } catch {
        /* read-only storage */
    }
}
