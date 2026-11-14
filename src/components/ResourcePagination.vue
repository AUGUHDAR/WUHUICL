<script setup lang="ts">
/** 资源管理组共享分页：六页同一形态（居中、上一页/N·共X项/下一页） */
defineProps<{ page: number; pageCount: number; total: number }>();
defineEmits<{ (e: 'update:page', v: number): void }>();
</script>

<template>
    <nav v-if="pageCount > 1" class="rp-pagination" aria-label="列表分页">
        <button
            class="btn btn-ghost"
            :disabled="page <= 1"
            @click="$emit('update:page', page - 1)"
        >
            上一页</button
        ><span class="muted">第 {{ page }} / {{ pageCount }} 页 · 共 {{ total }} 项</span
        ><button
            class="btn btn-ghost"
            :disabled="page >= pageCount"
            @click="$emit('update:page', page + 1)"
        >
            下一页
        </button>
    </nav>
</template>

<style scoped>
.rp-pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    padding: var(--space-2) 0;
    font-size: var(--text-sm);
}
</style>
