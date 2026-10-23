<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(defineProps<{ modelValue: number; disabled?: boolean }>(), {
    disabled: false
});
const emit = defineEmits<{ 'update:modelValue': [value: number] }>();
const percentage = computed(() =>
    Math.max(0, Math.min(100, Number.isFinite(props.modelValue) ? props.modelValue : 0))
);
function update(event: Event) {
    if (!props.disabled)
        emit('update:modelValue', Number((event.target as HTMLInputElement).value));
}
</script>

<template>
    <input
        class="percent-slider"
        type="range"
        min="0"
        max="100"
        step="1"
        :value="percentage"
        :disabled="disabled"
        :style="{ '--range-fill': percentage + '%' }"
        :aria-valuetext="percentage + '%'"
        @input="update"
    />
</template>

<style scoped>
/* 视觉与全局 .slider / .mem-slider 统一（PCL 规格）：细轨 + 空心圆点，只变色不放大。
   填充段仍用内联 --range-fill 渐变画在 input 背景上。 */
.percent-slider.percent-slider[type='range'] {
    appearance: none;
    box-sizing: border-box;
    display: block;
    flex: 1;
    width: 100%;
    min-width: 30px;
    height: 12px;
    margin: 9px 0;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: linear-gradient(
        to right,
        var(--accent) 0 var(--range-fill),
        color-mix(in srgb, var(--text-dim) 26%, transparent) var(--range-fill) 100%
    );
    background-repeat: no-repeat;
    cursor: pointer;
}
.percent-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 999px;
    background: transparent;
}
.percent-slider::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border: 2px solid var(--accent);
    border-radius: 50%;
    background: var(--card-solid, var(--card));
    transition:
        background 0.15s ease,
        border-color 0.15s ease;
}
.percent-slider:hover::-webkit-slider-thumb,
.percent-slider:active::-webkit-slider-thumb {
    background: var(--accent);
}
.percent-slider:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 5px;
}
.percent-slider:disabled {
    opacity: 0.5;
    cursor: default;
}
</style>
