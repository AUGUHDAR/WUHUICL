<script setup lang="ts">
/** @modrinth/ui Slider 兼容实现：range 输入 + 当前值（可带单位）气泡。 */
import { computed } from 'vue';

const props = withDefaults(
    defineProps<{
        modelValue: number;
        min?: number;
        max?: number;
        step?: number;
        unit?: string;
        disabled?: boolean;
    }>(),
    { min: 0, max: 100, step: 1, unit: undefined, disabled: false }
);
const emit = defineEmits<{ (e: 'update:modelValue', value: number): void }>();

const display = computed(() => `${props.modelValue}${props.unit ?? ''}`);
const percent = computed(() => {
    const span = props.max - props.min || 1;
    return Math.min(100, Math.max(0, ((props.modelValue - props.min) / span) * 100));
});
</script>

<template>
    <span class="lab-slider">
        <input
            type="range"
            :value="modelValue"
            :min="min"
            :max="max"
            :step="step"
            :disabled="disabled"
            :style="{ '--lab-slider-fill': percent + '%' }"
            @input="emit('update:modelValue', Number(($event.target as HTMLInputElement).value))"
        />
        <span class="lab-slider-value">{{ display }}</span>
    </span>
</template>
