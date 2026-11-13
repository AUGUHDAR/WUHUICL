<script setup lang="ts">
/** 工具行（UI_SYSTEM.md §5）：搜索/筛选/刷新同位等距。
 *  左主工具区 + 右次工具区，统一 gap；同布局按钮等距等宽等位由这里保证。 */
</script>

<template>
    <div class="toolbar-row">
        <div class="toolbar-row-main">
            <slot />
        </div>
        <div v-if="$slots.end" class="toolbar-row-end">
            <slot name="end" />
        </div>
    </div>
</template>

<style scoped>
.toolbar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
    min-width: 0;
}
.toolbar-row-main,
.toolbar-row-end {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    min-width: 0;
}
.toolbar-row-end {
    margin-left: auto;
    flex-shrink: 0;
}
</style>