<script setup lang="ts">
/** 背景 UI 区块容器（UI_SYSTEM.md §2/§5）：内容面 + 区块标题 + 可选折叠。
 *  同类功能多时 collapsible 默认收起（UX"折叠布局"）；折叠走原生 details，
 *  动效由 ui-system.css 的 details::details-content 契约统一提供。 */
withDefaults(
    defineProps<{
        title?: string;
        /** 折叠能力：true 时渲染为 details，defaultOpen 控制初始展开 */
        collapsible?: boolean;
        defaultOpen?: boolean;
        /** 去掉容器内边距（列表类内容自带行 padding 时用） */
        flush?: boolean;
    }>(),
    { collapsible: false, defaultOpen: true, flush: false }
);
</script>

<template>
    <details v-if="collapsible" class="section-card collapse" :open="defaultOpen || undefined">
        <summary class="collapse-head">
            <span class="group-title">{{ title }}</span>
            <span class="section-card-extra"><slot name="extra" /></span>
            <i class="collapse-arrow" aria-hidden="true"></i>
        </summary>
        <div class="collapse-body" :class="{ 'is-flush': flush }">
            <slot />
        </div>
    </details>
    <section v-else class="section-card" :class="{ 'is-flush': flush }">
        <div v-if="title || $slots.extra" class="section-card-head">
            <h2 v-if="title" class="group-title">{{ title }}</h2>
            <span class="section-card-extra"><slot name="extra" /></span>
        </div>
        <slot />
    </section>
</template>

<style scoped>
.section-card {
    background-color: var(--surface-content);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--card-pad);
    min-width: 0;
}
.section-card.is-flush {
    padding: 0;
}
.section-card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
}
.section-card-extra {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
}
/* 折叠态容器去内边距，由 collapse-head/body 自管（与 styles.css .collapse 对齐） */
.section-card.collapse {
    padding: 0;
}
.section-card.collapse .collapse-body.is-flush {
    padding: 0;
}
</style>