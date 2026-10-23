<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
withDefaults(
    defineProps<{
        label?: string;
        rows?: number;
        retry?: boolean;
        /** rows = 通用行式列表（默认）；grid = 卡片网格（社区资源搜索用，与结果卡片同构） */
        variant?: 'rows' | 'grid';
    }>(),
    {
        label: '正在加载…',
        rows: 5,
        retry: false,
        variant: 'rows'
    }
);
defineEmits<{ retry: [] }>();
const delayed = ref(false);
let timer: ReturnType<typeof setTimeout>;
onMounted(() => {
    timer = setTimeout(() => {
        delayed.value = true;
    }, 8000);
});
onUnmounted(() => clearTimeout(timer));
</script>
<template>
    <div class="content-skeleton" :class="'is-' + variant" role="status" aria-busy="true">
        <span class="muted">{{ label }}</span>
        <p v-if="delayed" class="muted">
            数据仍未返回，请检查网络或目录是否可用。<button
                v-if="retry"
                class="btn btn-ghost btn-sm"
                @click="$emit('retry')"
            >
                重新读取
            </button>
        </p>
        <!-- 行式：左方块 + 长条 + 右侧按钮（文件列表/服务器列表等） -->
        <template v-if="variant === 'rows'">
            <div v-for="row in rows" :key="row" class="skeleton-row" aria-hidden="true">
                <i /><span /><b />
            </div>
        </template>
        <!-- 网格：与社区资源结果卡片同构（46px 图标 + 两行标题 + 描述 + 底部按钮条） -->
        <div v-else class="skeleton-grid" aria-hidden="true">
            <div v-for="card in rows" :key="card" class="skeleton-card">
                <div class="skeleton-card-top"><i /><div><span /><span class="short" /></div></div>
                <span class="desc" /><span class="desc short" />
                <div class="skeleton-card-foot"><span /><b /></div>
            </div>
        </div>
    </div>
</template>
<style scoped>
.content-skeleton {
    display: grid;
    gap: 12px;
    padding: 16px 0;
}
.skeleton-row {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 48px;
}
.skeleton-row i,
.skeleton-row span,
.skeleton-row b {
    background: color-mix(in srgb, var(--text) 9%, var(--card));
    border-radius: var(--radius-sm);
}
.skeleton-row i {
    width: 36px;
    height: 36px;
}
.skeleton-row span {
    flex: 1;
    height: 18px;
    max-width: 65%;
}
.skeleton-row b {
    margin-left: auto;
    width: 76px;
    height: 32px;
}

/* 网格变体：列宽/间距与社区资源结果网格一致（auto-fit, minmax(300px), gap 16px） */
.skeleton-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr));
    gap: 16px;
}
.skeleton-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    background: var(--card);
}
.skeleton-card i,
.skeleton-card span,
.skeleton-card b {
    display: block;
    background: color-mix(in srgb, var(--text) 9%, var(--card));
    border-radius: var(--radius-sm);
}
.skeleton-card-top {
    display: flex;
    align-items: center;
    gap: 12px;
}
.skeleton-card-top > i {
    width: 46px;
    height: 46px;
    flex: none;
    border-radius: var(--radius-md, var(--radius));
}
.skeleton-card-top > div {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 8px;
}
.skeleton-card-top span {
    height: 16px;
}
.skeleton-card-top span.short,
.skeleton-card span.short,
.skeleton-card-foot span {
    width: 45%;
}
.skeleton-card .desc {
    height: 14px;
}
.skeleton-card-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: auto;
    padding-top: 12px;
    border-top: 1px solid var(--border);
}
.skeleton-card-foot span {
    height: 14px;
}
.skeleton-card-foot b {
    width: 76px;
    height: 32px;
}
</style>
