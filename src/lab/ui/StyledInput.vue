<script setup lang="ts">
/**
 * @modrinth/ui StyledInput 兼容实现：图标 + 原生 input + 可清空。
 * 暴露 focus()（页面用 ref="searchInput" 聚焦搜索框）。
 */
import { ref } from 'vue';
import type { Component } from 'vue';

const props = withDefaults(
    defineProps<{
        modelValue: string;
        type?: string;
        placeholder?: string;
        icon?: Component;
        prefix?: string;
        clearable?: boolean;
        disabled?: boolean;
        wrapperClass?: string;
    }>(),
    { type: 'text', placeholder: undefined, icon: undefined, prefix: undefined, clearable: false, disabled: false, wrapperClass: undefined }
);
const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>();

const input = ref<HTMLInputElement>();
function focus() {
    input.value?.focus();
}
defineExpose({ focus, el: input });
</script>

<template>
    <span class="lab-input" :class="wrapperClass">
        <component :is="icon" v-if="icon" class="lab-input-icon" aria-hidden="true" />
        <span v-if="prefix" class="lab-input-prefix">{{ prefix }}</span>
        <input
            ref="input"
            :type="type"
            :value="modelValue"
            :placeholder="placeholder"
            :disabled="disabled"
            @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        />
        <button
            v-if="clearable && modelValue"
            type="button"
            class="lab-input-clear"
            aria-label="清空"
            @click="emit('update:modelValue', ''); focus()"
        >
            ×
        </button>
    </span>
</template>
