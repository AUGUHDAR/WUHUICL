<script setup lang="ts">
/** @modrinth/ui Toggle 兼容实现：小号开关。 */
defineProps<{ modelValue: boolean; small?: boolean; disabled?: boolean }>();
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>();
</script>

<template>
    <button
        type="button"
        class="lab-toggle"
        :class="{ on: modelValue, small }"
        role="switch"
        :aria-checked="modelValue"
        :disabled="disabled"
        @click="!disabled && emit('update:modelValue', !modelValue)"
    >
        <span class="lab-toggle-thumb" aria-hidden="true"></span>
    </button>
</template>
