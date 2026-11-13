<script setup lang="ts">
/**
 * 投影行缩略预览图：进入视口才排队渲染（isometric），渲染完成前显示格式占位图标。
 * 结果按条目 id 缓存在模块级（会话内二次进入立即显示）。
 */
import { onMounted, onUnmounted, ref } from 'vue';
import type { SchematicPreviewSource } from '../lab/schematic-preview/backend';
import {
    cachedSchematicThumbnail,
    requestSchematicThumbnail,
} from '../lab/schematic-preview/thumbnail';

const props = defineProps<{
    id: string;
    source: SchematicPreviewSource | null;
}>();

const src = ref(cachedSchematicThumbnail(props.id));
let observer: IntersectionObserver | undefined;
let active = true;

function finish(dataUrl: string | null) {
    if (!active) return;
    if (dataUrl) src.value = dataUrl;
}

const el = ref<HTMLElement>();

onMounted(() => {
    if (src.value || !props.source) return;
    observer = new IntersectionObserver(
        (entries) => {
            if (!entries.some((entry) => entry.isIntersecting)) return;
            observer?.disconnect();
            observer = undefined;
            requestSchematicThumbnail(props.id, props.source!, finish);
        },
        { rootMargin: '120px' }
    );
    if (el.value) observer.observe(el.value);
});

onUnmounted(() => {
    observer?.disconnect();
    active = false;
});
</script>

<template>
    <span ref="el" class="projection-thumb-slot">
        <img v-if="src" class="projection-thumb-img" :src="src" alt="" draggable="false" />
        <svg
            v-else
            class="projection-thumb-fallback"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="M21 8 12 3 3 8v8l9 5 9-5Z" />
            <path d="m3 8 9 5 9-5" />
            <path d="M12 13v8" />
        </svg>
    </span>
</template>

<style scoped>
.projection-thumb-slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 52px;
    height: 52px;
    border-radius: var(--radius-sm);
    background: var(--card-2);
    overflow: hidden;
}
.projection-thumb-img {
    width: 100%;
    height: 100%;
    object-fit: contain;
}
.projection-thumb-fallback {
    width: 24px;
    height: 24px;
    color: var(--text-dim);
}
</style>
