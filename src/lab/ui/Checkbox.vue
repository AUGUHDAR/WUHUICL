<script setup lang="ts">
/** @modrinth/ui Checkbox 兼容实现：label + description 两行文本。 */
defineProps<{
    modelValue?: boolean;
    label?: string;
    description?: string;
    disabled?: boolean;
}>();
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>();
</script>

<template>
    <label class="lab-check" :class="{ disabled }">
        <input
            type="checkbox"
            :checked="!!modelValue"
            :disabled="disabled"
            @change="emit('update:modelValue', ($event.target as HTMLInputElement).checked)"
        />
        <span class="lab-check-box" aria-hidden="true"></span>
        <span v-if="label || description" class="lab-check-text">
            <span v-if="label" class="lab-check-label">{{ label }}</span>
            <span v-if="description" class="lab-check-description">{{ description }}</span>
        </span>
        <slot v-else />
    </label>
</template>
