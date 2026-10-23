<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
    defineProps<{ name: string; size?: number; filled?: boolean }>(),
    { size: 20, filled: false }
);

// 加载器/版本类型走真实图标（HMCL 官方同款素材，public/loaders/）；
// 其余保持内置线性图形
const realLogos: Record<string, string> = {
    forge: 'loaders/forge.png',
    fabric: 'loaders/fabric.png',
    quilt: 'loaders/quilt.png',
    neoforge: 'loaders/neoforge.png',
    optifine: 'loaders/optifine.png',
    vanilla: 'loaders/grass.png',
    snapshot: 'loaders/command.png'
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
    warning: 'M12 3 2.5 20h19L12 3zM12 9.5v5M12 17.5h.01',
    star: 'm12 3 2.7 5.8 6.3.8-4.6 4.3 1.2 6.1-5.6-3.1-5.6 3.1 1.2-6.1L3 9.6l6.3-.8z',
    clip: 'm21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48',
    comment: 'M7.9 20A9 9 0 1 0 4 16.1L2 22Z',
    none: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M6 6l12 12'
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
