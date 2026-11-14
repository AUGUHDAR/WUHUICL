<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
withDefaults(
    defineProps<{
        label?: string;
        rows?: number;
        retry?: boolean;
        /** rows = 通用行式列表（默认）；grid = 卡片网格（社区资源搜索用）；groups = 折叠分组列表（版本下载页用，与分组+行结构同构） */
        variant?: 'rows' | 'grid' | 'groups';
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
        <!-- 分组：组头（图标+标题+徽标）+ 若干行（图标+名称+类型+按钮），与版本下载页同构 -->
        <template v-else-if="variant === 'groups'">
            <div
                v-for="g in Math.max(2, Math.round(rows / 4))"
                :key="g"
                class="skeleton-group"
                aria-hidden="true"
            >
                <div class="skeleton-group-head">
                    <i class="gi" /><span class="title" /><span class="chip" />
                </div>
                <div v-for="r in 4" :key="r" class="skeleton-group-row">
                    <i class="icon" /><span class="line" /><span class="tag" /><b class="btn" />
                </div>
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

/* 分组变体：组头 22px 图标 + 标题 + 徽标；行 30px 图标 + 名称 + 类型徽标 + 按钮（对齐版本下载页行） */
.skeleton-group {
    display: grid;
    gap: 4px;
}
.skeleton-group i,
.skeleton-group span,
.skeleton-group b {
    display: block;
    background: color-mix(in srgb, var(--text) 9%, var(--card));
    border-radius: var(--radius-sm);
}
.skeleton-group-head {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 4px;
}
.skeleton-group-head .gi {
    width: 22px;
    height: 22px;
}
.skeleton-group-head .title {
    width: 150px;
    height: 15px;
}
.skeleton-group-head .chip {
    width: 64px;
    height: 14px;
    border-radius: 999px;
}
.skeleton-group-row {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 44px;
    padding: 0 12px;
}
.skeleton-group-row .icon {
    width: 30px;
    height: 30px;
}
.skeleton-group-row .line {
    width: 190px;
    height: 14px;
}
.skeleton-group-row .tag {
    width: 46px;
    height: 18px;
    border-radius: 999px;
}
.skeleton-group-row .btn {
    margin-left: auto;
    width: 64px;
    height: 28px;
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
    /* 预加载骨架与结果卡同表面（surface-solid 实底）：模糊只在 .shell 一层，
     * 半透卡面会让骨架态的缝隙透出更深一层（与加载后同 bug 同源）。 */
    background: var(--surface-solid);
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
