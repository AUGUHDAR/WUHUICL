<script setup lang="ts">
import { nextTick } from 'vue';

const props = defineProps<{
    tab: 'browse' | 'favorites';
    favoritesCount: number;
}>();
const emit = defineEmits<{ change: [tab: 'browse' | 'favorites'] }>();

function select(tab: 'browse' | 'favorites') {
    emit('change', tab);
}

function onKeydown(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next =
        event.key === 'Home'
            ? 'browse'
            : event.key === 'End'
              ? 'favorites'
              : props.tab === 'browse'
                ? 'favorites'
                : 'browse';
    emit('change', next);
    void nextTick(() => document.querySelector<HTMLElement>(`[data-ui="community:${next}"]`)?.focus());
}
</script>

<template>
    <div
        class="community-sections"
        role="tablist"
        aria-label="社区资源分区"
        @keydown="onKeydown"
    >
        <button
            class="community-section"
            role="tab"
            data-ui="community:browse"
            :tabindex="tab === 'browse' ? 0 : -1"
            :aria-selected="tab === 'browse'"
            :class="{ active: tab === 'browse' }"
            @click="select('browse')"
        >
            找资源</button
        ><button
            class="community-section"
            role="tab"
            data-ui="community:favorites"
            :tabindex="tab === 'favorites' ? 0 : -1"
            :aria-selected="tab === 'favorites'"
            :class="{ active: tab === 'favorites' }"
            @click="select('favorites')"
        >
            已收藏 MOD <span>{{ favoritesCount }}</span>
        </button>
    </div>
</template>

<style scoped>
.community-sections {
    display: flex;
    gap: 6px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card);
}
.community-section {
    flex: 1;
    min-height: 38px;
    padding: 8px 16px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
}
.community-section.active {
    background: var(--accent-soft);
    color: var(--accent-2);
    font-weight: 600;
}
.community-section span {
    margin-left: 4px;
    font-size: 11px;
}
.community-section:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
}
</style>
