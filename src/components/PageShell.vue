<script setup lang="ts">
/** 全站统一页头（UI_SYSTEM.md §4/§5）：左标题+副标题，右页级操作位。
 *  标题用 h1 保证读屏层级；操作位按钮等距等宽由 ToolbarRow/调用方保证。 */
defineProps<{
    title: string;
    subtitle?: string;
}>();
</script>

<template>
    <header class="page-shell-head">
        <div class="page-shell-titles">
            <h1 class="page-title">{{ title }}</h1>
            <p v-if="subtitle" class="page-sub">{{ subtitle }}</p>
        </div>
        <div v-if="$slots.actions" class="page-shell-actions">
            <slot name="actions" />
        </div>
    </header>
</template>

<style scoped>
.page-shell-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-2);
}
.page-shell-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
}
.page-shell-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    flex-shrink: 0;
}
</style>