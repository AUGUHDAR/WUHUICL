<script setup lang="ts">
/** 全站统一分类 tab（UI_SYSTEM.md §5）：每颗各自带边框的药丸，不做整体包裹容器
 *  （形态抽自 InstanceCenter .ic-tabs 已拍板基线）。count >= 0 时渲染角标。 */
export interface TabPillItem {
    value: string;
    label: string;
    count?: number;
    disabled?: boolean;
    /** 禁用原因：置灰 + tooltip 说明原因与恢复方式（UX"联动禁用要显式"） */
    disabledReason?: string;
    /** 悬浮说明（非禁用态的补充说明） */
    title?: string;
    /** 红点标记（未读/需注意）；与 count 互斥优先红点 */
    dot?: boolean;
}
defineProps<{
    items: TabPillItem[];
    modelValue: string;
    ariaLabel?: string;
}>();
const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>();
</script>

<template>
    <div class="tab-pills" role="tablist" :aria-label="ariaLabel">
        <button
            v-for="tab in items"
            :key="tab.value"
            class="tab-pill"
            :class="{ active: modelValue === tab.value }"
            role="tab"
            :aria-selected="modelValue === tab.value"
            :disabled="tab.disabled"
            :title="tab.disabled ? tab.disabledReason : tab.title"
            @click="!tab.disabled && emit('update:modelValue', tab.value)"
        >
            {{ tab.label
            }}<span v-if="tab.dot" class="tab-pill-dot" aria-label="有更新"></span
            ><span
                v-else-if="tab.count !== undefined && tab.count >= 0"
                class="tab-pill-count"
                >{{ tab.count }}</span
            >
        </button>
    </div>
</template>

<style scoped>
.tab-pills {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    flex-shrink: 0;
}
.tab-pill {
    background: var(--card-2);
    border: 1px solid var(--border);
    color: var(--text-dim);
    padding: var(--space-2) 20px; /* 20px = 药丸形态的水平基底（形状常数，同 999px 圆角） */
    white-space: nowrap;
    border-radius: 999px;
    font-size: var(--text-sm);
    font-family: inherit;
    cursor: pointer;
    transition:
        background 0.15s ease,
        color 0.15s ease,
        border-color 0.15s ease;
}
.tab-pill:hover:not(:disabled):not(.active) {
    color: var(--text);
    border-color: var(--border-strong);
}
.tab-pill.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
}
.tab-pill:disabled {
    opacity: 0.5;
    cursor: not-allowed;
}
.tab-pill-count {
    margin-left: var(--space-2);
    font-size: var(--text-xs);
    opacity: 0.75;
}
.tab-pill-dot {
    margin-left: var(--space-2);
    width: 7px;
    height: 7px;
    border-radius: 999px;
    background: var(--danger);
    display: inline-block;
}
</style>