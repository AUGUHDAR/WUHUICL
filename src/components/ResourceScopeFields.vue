<script setup lang="ts">
/**
 * 资源页统一的「内容范围 + 游戏实例」选择字段：与文件管理器同排版（标签在上的两个下拉）。
 * 所有按实例/文件夹管理资源的页面一律复用本组件，保证同义控件跨页同位同样式。
 *
 * 联动禁用（消除歧义）：范围选「版本文件夹内的所有」时视图聚合全部实例，
 * 实例选择器随之禁用；置灰 + 悬浮说明原因与恢复方式（不可用 ≠ 消失）。
 */
import SelectMenu from './SelectMenu.vue';

defineProps<{
    scope: string;
    scopeOptions: Array<{ value: string; label: string; description?: string }>;
    instanceId: string;
    instanceOptions: Array<{ value: string; label: string; description?: string }>;
    /** 范围为聚合视图时锁死实例选择（父组件按自身语义传入） */
    instanceLocked?: boolean;
    disabled?: boolean;
}>();
defineEmits<{
    (e: 'update:scope', value: string): void;
    (e: 'update:instanceId', value: string): void;
}>();

const LOCKED_HINT = '此范围已聚合全部实例，无需选择实例；切回「当前实例」即可选择';
</script>

<template>
    <label class="rsf-field">
        <span>内容范围</span
        ><SelectMenu
            :model-value="scope"
            :options="scopeOptions"
            :disabled="disabled"
            @update:model-value="$emit('update:scope', $event)"
        />
    </label>
    <label class="rsf-field" :title="instanceLocked ? LOCKED_HINT : undefined">
        <span>游戏实例</span
        ><SelectMenu
            :model-value="instanceId"
            :options="instanceOptions"
            :disabled="disabled || instanceLocked"
            :placeholder="instanceLocked ? '全部实例' : '选择游戏实例'"
            aria-label="游戏实例"
            @update:model-value="$emit('update:instanceId', $event)"
        />
    </label>
</template>

<style scoped>
.rsf-field {
    display: grid;
    gap: 6px;
    flex: 0 1 440px;
    min-width: 0;
    font-size: 12px;
    color: var(--text-dim);
}
.rsf-field :deep(.select-menu-btn) {
    width: 100%;
    max-width: none;
}
</style>
