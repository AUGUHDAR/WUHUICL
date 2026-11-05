<script setup lang="ts">
/** @modrinth/ui Tabs 兼容实现：value + tabs[{value,label,icon}]，下划线式页签。 */
import type { Component } from 'vue';

type Tab = { value: string; label: string; icon?: Component };

defineProps<{ value: string; tabs: Tab[] }>();
const emit = defineEmits<{ (e: 'update:value', value: string): void }>();
</script>

<template>
    <div class="lab-tabs" role="tablist">
        <button
            v-for="tab in tabs"
            :key="tab.value"
            type="button"
            class="lab-tab"
            role="tab"
            :aria-selected="tab.value === value"
            :class="{ active: tab.value === value }"
            @click="emit('update:value', tab.value)"
        >
            <component :is="tab.icon" v-if="tab.icon" aria-hidden="true" />
            <span v-if="tab.label">{{ tab.label }}</span>
        </button>
    </div>
</template>
