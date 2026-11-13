<script setup lang="ts">
import UiGlyph from './UiGlyph.vue';
/** 空状态（UX"空状态指引"）：说明这里以后放什么 + 现在怎么填进来（附入口按钮）。
 *  图标走 UiGlyph 统一矢量语言；入口按钮由调用方通过 action 插槽提供。 */
defineProps<{
    title: string;
    description?: string;
    /** UiGlyph 图标名；不传则不渲染图标 */
    icon?: string;
}>();
</script>

<template>
    <div class="empty-state">
        <UiGlyph v-if="icon" :name="icon" :size="32" class="empty-state-icon" />
        <p class="empty-state-title">{{ title }}</p>
        <p v-if="description" class="empty-state-desc">{{ description }}</p>
        <div v-if="$slots.action" class="empty-state-action">
            <slot name="action" />
        </div>
    </div>
</template>

<style scoped>
.empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    min-height: 120px;
    padding: var(--space-7) var(--space-5);
    text-align: center;
    color: var(--text-dim);
}
.empty-state-icon {
    opacity: 0.6;
}
.empty-state-title {
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text);
}
.empty-state-desc {
    font-size: var(--text-sm);
    line-height: 1.7;
    max-width: 420px;
}
.empty-state-action {
    margin-top: var(--space-2);
}
</style>