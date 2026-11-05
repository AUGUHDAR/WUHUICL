<script setup lang="ts">
/**
 * @modrinth/ui Accordion 兼容实现：#title/#button 插槽 + 默认内容，可折叠（默认收起）。
 * 生物群系选择器按分类分组用。
 */
import { ref } from 'vue';

withDefaults(
    defineProps<{
        openByDefault?: boolean;
        buttonClass?: string;
        contentClass?: string;
        forceOpen?: boolean;
    }>(),
    { openByDefault: false, buttonClass: undefined, contentClass: undefined, forceOpen: false }
);
const toggled = ref(false);
</script>

<template>
    <div class="lab-accordion" :class="{ open: forceOpen || openByDefault || toggled }">
        <button
            type="button"
            :class="buttonClass"
            class="lab-accordion-trigger"
            :aria-expanded="forceOpen || openByDefault || toggled"
            @click="!forceOpen && (toggled = !toggled)"
        >
            <slot name="button" :open="forceOpen || openByDefault || toggled">
                <slot name="title" :open="forceOpen || openByDefault || toggled" />
            </slot>
            <svg
                v-if="!forceOpen"
                class="lab-accordion-chevron"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
            >
                <path d="m6 9 6 6 6-6" />
            </svg>
        </button>
        <div v-show="forceOpen || openByDefault || toggled" :class="contentClass" class="lab-accordion-content">
            <slot />
        </div>
    </div>
</template>
