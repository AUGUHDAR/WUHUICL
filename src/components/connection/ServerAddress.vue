<script setup lang="ts">
import UiGlyph from '../UiGlyph.vue';
defineProps<{ address: string; revealed: boolean; copyable?: boolean }>();
defineEmits<{ toggle: []; copy: [] }>();
</script>

<template>
    <span class="server-address-line">
        <span v-if="revealed" class="server-address-value mono">{{ address }}</span>
        <span v-else class="server-address-mask">地址已隐藏</span>
        <button
            type="button"
            class="server-address-toggle"
            :aria-label="revealed ? '隐藏服务器地址' : '显示服务器地址'"
            :aria-pressed="revealed"
            :title="revealed ? '隐藏服务器地址' : '显示服务器地址'"
            @click.stop="$emit('toggle')"
            @dblclick.stop
        >
            <!-- 手绘眼睛 → UiGlyph 统一口径；显隐状态由文字「显示/隐藏」+ aria-pressed 表达 -->
            <UiGlyph name="eye" :size="14" />
            <span>{{ revealed ? '隐藏' : '显示' }}</span>
        </button>
        <button
            v-if="copyable && revealed"
            type="button"
            class="server-address-toggle"
            aria-label="复制服务器地址"
            title="复制服务器地址"
            @click.stop="$emit('copy')"
        >
            复制
        </button>
    </span>
</template>

<style scoped>
.server-address-line {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    min-width: 0;
    color: var(--text-dim);
    font-size: var(--text-xs);
    pointer-events: none;
}
.server-address-value {
    overflow-wrap: anywhere;
    min-width: 0;
}
.server-address-mask {
    letter-spacing: 0.02em;
}
.server-address-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    min-height: 28px;
    padding: 2px 7px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--card-2);
    color: var(--text-dim);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
    pointer-events: auto;
}
.server-address-toggle svg {
    width: 14px;
    height: 14px;
}
.server-address-toggle:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent-2);
}
.server-address-toggle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
}
</style>
