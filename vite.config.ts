import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { visualIds } from './scripts/visual-ids';
import { resolve } from 'node:path';
import { readFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync(resolve(__dirname, 'package.json'), 'utf-8'));
const NOTICES = resolve(__dirname, 'THIRD_PARTY_NOTICES.md');

export default defineConfig({
    // 渲染层与 KAMUCL 1:1：两个入口（主界面 + 启动闪屏），同一套源码
    build: {
        outDir: 'dist',
        rollupOptions: {
            input: {
                index: resolve(__dirname, 'index.html'),
                splash: resolve(__dirname, 'splash.html')
            }
        },
        target: 'chrome120'
    },
    plugins: [visualIds(), vue()],
    define: {
        __APP_VERSION__: JSON.stringify(pkg.version)
    },
    resolve: {
        alias: {
            '@shared': resolve(__dirname, 'shared'),
            // ThirdPartyNotices.vue 原文用 ../../../../THIRD_PARTY_NOTICES.md?raw（electron-vite 工程根）；
            // 这里保持渲染层零改动，把该确切说明符映射到本地副本。
            '../../../../THIRD_PARTY_NOTICES.md?raw': `${NOTICES}?raw`
        }
    },
    server: {
        port: 5173,
        strictPort: true,
        watch: {
            ignored: ['**/src-tauri/**']
        }
    }
});
