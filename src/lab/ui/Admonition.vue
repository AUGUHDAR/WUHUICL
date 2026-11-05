<script setup lang="ts">
/** @modrinth/ui Admonition 兼容实现（两个工具只用到 critical 提示条）。 */
withDefaults(defineProps<{ type?: 'critical' | 'warning' | 'info'; header?: string; body?: string }>(), {
    type: 'info',
    header: undefined,
    body: undefined
});
</script>

<template>
    <div class="lab-admonition" :class="type" role="alert">
        <p v-if="header" class="lab-admonition-header">{{ header }}</p>
        <p v-if="body" class="lab-admonition-body">{{ body }}</p>
        <slot />
    </div>
</template>
