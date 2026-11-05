<script setup lang="ts">
/**
 * @modrinth/ui Button 兼容实现：type/color/size/circular/icon-only 映射到 wuhuicl 按钮样式。
 */
import { computed, useAttrs } from 'vue';

defineOptions({ inheritAttrs: false });

const props = withDefaults(
    defineProps<{
        type?: 'base' | 'outlined' | 'quiet' | 'colored' | 'chip-text';
        color?: 'brand' | 'red' | undefined;
        size?: '2xs' | undefined;
        circular?: boolean;
        iconOnly?: boolean;
        nativeType?: string;
        label?: string;
        disabled?: boolean;
    }>(),
    {
        type: 'base',
        color: undefined,
        size: undefined,
        circular: false,
        iconOnly: false,
        nativeType: 'button',
        label: undefined,
        disabled: false
    }
);

const attrs = useAttrs();
const classes = computed(() => {
    const list = ['btn'];
    if (props.size === '2xs') list.push('btn-sm');
    if (props.iconOnly) list.push('lab-btn-icon');
    switch (props.type) {
        case 'base':
            list.push('btn-gold');
            break;
        case 'colored':
            list.push(props.color === 'brand' ? 'btn-gold' : 'btn-ghost');
            break;
        case 'outlined':
            list.push(props.color === 'red' ? 'btn-danger' : 'btn-ghost');
            break;
        case 'quiet':
            list.push(props.color === 'red' ? 'btn-danger' : 'btn-text');
            break;
        case 'chip-text':
            list.push('lab-btn-chip');
            if (props.color === 'brand') list.push('active');
            break;
    }
    return list;
});
const rest = computed(() => {
    const { class: _class, 'aria-pressed': _pressed, 'aria-label': _label, ...others } = attrs;
    return others;
});
const pressed = computed(() => attrs['aria-pressed'] as boolean | undefined);
const ariaLabel = computed(
    () => props.label ?? (attrs['aria-label'] as string | undefined)
);
</script>

<template>
    <button
        :class="[classes, attrs.class]"
        :type="nativeType"
        :aria-pressed="pressed"
        :aria-label="ariaLabel"
        :disabled="disabled"
        v-bind="rest"
    >
        <slot />
    </button>
</template>
