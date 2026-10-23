/// <reference types="vite/client" />

/** 构建时注入的启动器版本号（来自 package.json） */
declare const __APP_VERSION__: string;

declare module '*.vue' {
    import type { DefineComponent } from 'vue';
    const component: DefineComponent<object, object, unknown>;
    export default component;
}

/** preload 暴露给渲染进程的桥接 API */
interface KamuclBridge {
    invoke: (channel: string, ...args: unknown[]) => Promise<unknown>;
    on: (channel: string, cb: (...args: unknown[]) => void) => () => void;
    send: (channel: string, ...args: unknown[]) => void;
    getFilePath: (file: File) => string;
    platform: string;
}

interface Window {
    kamucl: KamuclBridge;
    /** 主进程注入的自定义开屏头像 data URL；空串/未注入 = 用内置默认图 */
    __KAMUCL_SPLASH_FACE__?: string;
    kamuclSplash: {
        ready(): void;
        assembled(): void;
        finished(): void;
        failed(message: string): void;
        onPointer(callback: (point: { x: number; y: number }) => void): () => void;
        onState(callback: (state: import('../../shared/startup').BootState) => void): () => void;
        onReveal(callback: () => void): () => void;
    };
}
