<script setup lang="ts">
/**
 * @modrinth/ui Combobox 兼容实现：两个工具只用「单选下拉」，直接复用 wuhuicl SelectMenu
 * （浮层 Teleport，主题一致）。SelectMenu 只收 string 值，这里对 number 值做双向桥接。
 */
import { computed } from 'vue';
import SelectMenu from '../../components/SelectMenu.vue';

const props = defineProps<{
    modelValue: string | number;
    options: Array<{ value: string | number; label: string }>;
    disabled?: boolean;
    placeholder?: string;
}>();
const emit = defineEmits<{ (e: 'update:modelValue', value: string | number): void }>();

const keyed = computed(() =>
    props.options.map((option) => ({ value: String(option.value), label: option.label }))
);
function onInput(value: string) {
    const match = props.options.find((option) => String(option.value) === value);
    emit('update:modelValue', match ? match.value : value);
}
</script>

<template>
    <SelectMenu
        :model-value="String(modelValue)"
        :options="keyed"
        :disabled="disabled"
        :placeholder="placeholder"
        @update:model-value="onInput"
    />
</template>
