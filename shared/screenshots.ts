import type { InstanceTarget } from './instanceCenter';

/** 截图条目（与 screenshots.rs scan 的 json 字段一一对应） */
export interface ScreenshotEntry {
    folder?: string;
    /** 归属实例 id（来自实例 screenshots/ 的记录才有；未隔离实例共用目录的记录没有） */
    instanceId?: string;
    id: string;
    name: string;
    size: number;
    modified: number;
    /** 展示用来源标签：实例 id 或「未隔离实例共用」 */
    source: string;
    directory: string;
    /** 来自版本文件夹根的 screenshots（版本隔离关闭时，未隔离实例的共用目录） */
    shared: boolean;
}

export interface ScreenshotCatalog {
    entries: ScreenshotEntry[];
    warnings: string[];
    /** 当前版本文件夹根（空 = 未绑定游戏文件夹） */
    folderRoot: string;
    instances: Array<InstanceTarget & { name: string; isolated: boolean }>;
}

export interface ScreenshotRequest {
    action: 'copy' | 'export' | 'trash';
    ids: string[];
    target?: InstanceTarget;
}

export interface ScreenshotResult {
    id: string;
    name: string;
    ok: boolean;
    path?: string;
    error?: string;
}

/** MC 截图位于游戏工作目录的 screenshots/：
 * 隔离实例 = versions/<id>/screenshots；未隔离实例 = 版本文件夹根 screenshots（共用） */
export const SCREENSHOT_DIR = 'screenshots';
