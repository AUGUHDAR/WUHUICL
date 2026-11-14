<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(defineProps<{ name: string; size?: number; filled?: boolean }>(), {
    size: 20,
    filled: false
});

// 加载器/版本类型走真实图标（HMCL 官方同款素材，public/loaders/）；
// 其余保持内置线性图形
const realLogos: Record<string, string> = {
    forge: 'loaders/forge.png',
    fabric: 'loaders/fabric.png',
    quilt: 'loaders/quilt.png',
    neoforge: 'loaders/neoforge.png',
    optifine: 'loaders/optifine.png',
    vanilla: 'loaders/grass.png',
    snapshot: 'loaders/command.png',
    stone: 'loaders/stone.png'
};
const logoSrc = computed(() => realLogos[props.name] ?? '');

const paths: Record<string, string> = {
    brush: 'm15 5 4 4M4 20l4-1L20 7a2.8 2.8 0 0 0-4-4L4 15z',
    erase: 'm4 14 9-10a2 2 0 0 1 3 0l5 5a2 2 0 0 1 0 3L13 21H8zM8 10l9 8M13 21h8',
    pick: 'm15 4 5 5M14 3l7 7M16 8 5 19H3v-2L14 6',
    fill: 'm5 11 7-7 8 8-8 8-8-8zM8 2l4 4M20 15s-3 3-3 5a3 3 0 0 0 6 0c0-2-3-5-3-5',
    undo: 'M8 4 3 9l5 5M3 9h10a7 7 0 0 1 0 14',
    redo: 'm16 4 5 5-5 5M21 9H11a7 7 0 0 0 0 14',
    cube: 'm12 3 9 5v9l-9 5-9-5V8zM3 8l9 5 9-5M12 13v9M7 5l10 6',
    eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
    sun: 'M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M5 19l1-1M18 6l1-1',
    expand: 'M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5',
    file: 'M14 2H5v20h14V7zM14 2v5h5',
    image: 'M3 3h18v18H3zM3 17l6-6 4 4 3-3 5 5M8 7h.01',
    folder: 'M2 6h8l2 3h10v12H2z',
    download: 'M12 2v13m-5-5 5 5 5-5M3 16v5h18v-5',
    upload: 'M12 16V3m-5 5 5-5 5 5M3 16v5h18v-5',
    close: 'm5 5 14 14M19 5 5 19',
    check: 'm5 12.5 4.5 4.5L19 7',
    // 快照：倒带/历史语义，比通用 clock 更能表达「回到过去」
    clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7.5V12l3.2 2',
    warning: 'M12 3 2.5 20h19L12 3zM12 9.5v5M12 17.5h.01',
    star: 'm12 3 2.7 5.8 6.3.8-4.6 4.3 1.2 6.1-5.6-3.1-5.6 3.1 1.2-6.1L3 9.6l6.3-.8z',
    clip: 'm21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48',
    comment: 'M7.9 20A9 9 0 1 0 4 16.1L2 22Z',
    none: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M6 6l12 12',
    // 全站通用操作符号（2026-11-14 收编文本符号/手绘 SVG 的落点）
    bell: 'M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0',
    refresh: 'M21 12a9 9 0 1 1-2.64-6.36L21 8M21 3v5h-5',
    'arrow-left': 'M19 12H5M12 19l-7-7 7-7',
    'arrow-right': 'M5 12h14M12 5l7 7-7 7',
    'chevron-left': 'm15 18-6-6 6-6',
    'chevron-right': 'm9 18 6-6-6-6',
    'chevron-down': 'm6 9 6 6 6-6',
    gear: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z',
    search: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.35-4.35',
    grid: 'M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z',
    plus: 'M12 5v14M5 12h14',
    minus: 'M5 12h14',
    // 三点「更多」：0 长线段 + round cap 即圆点
    more: 'M5 12h.01M12 12h.01M19 12h.01',
    play: 'm7 4 12 8-12 8z',
    trash: 'M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 11v6M14 11v6',
    globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z',
    book: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20V2H6.5A2.5 2.5 0 0 0 4 4.5v15zM4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5M9 7h7',
    'eye-off': 'M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 10 8 10 8a13.16 13.16 0 0 1-1.67 2.68M6.61 6.61A13.53 13.53 0 0 0 2 12s3 8 10 8a9.74 9.74 0 0 0 5.39-1.61M2 2l20 20M14.12 14.12a3 3 0 1 1-4.24-4.24'
};
const path = computed(() => paths[props.name] || paths.cube);
</script>
<template>
    <img
        v-if="logoSrc"
        class="ui-glyph-logo"
        :src="logoSrc"
        :width="size"
        :height="size"
        alt=""
        aria-hidden="true"
    />
    <svg
        v-else
        :width="size"
        :height="size"
        viewBox="0 0 24 24"
        :fill="filled ? 'currentColor' : 'none'"
        :stroke-width="filled ? 1 : 1.7"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        focusable="false"
    >
        <path :d="path" />
    </svg>
</template>
<style scoped>
.ui-glyph-logo {
    object-fit: contain;
    display: inline-block;
    vertical-align: -0.15em;
    border-radius: 3px;
}
</style>
