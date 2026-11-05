<script setup lang="ts">
/**
 * @modrinth/ui PopoutMenu 兼容实现：触发按钮（默认插槽，样式类经 $attrs 落在按钮上）
 * + 浮出菜单（#menu 插槽，Teleport 到 container 或 body）。点击外部 / Esc 关闭；
 * placement 支持 bottom-* / top-*（视口放不下时自动翻转）。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useAttrs } from 'vue';

defineOptions({ inheritAttrs: false });

const props = withDefaults(
    defineProps<{
        placement?: string;
        dropdownClass?: string;
        container?: string | HTMLElement | boolean;
        ariaLabel?: string;
        disabled?: boolean;
    }>(),
    {
        placement: 'bottom-end',
        dropdownClass: undefined,
        container: undefined,
        ariaLabel: undefined,
        disabled: false
    }
);

const attrs = useAttrs();
const open = ref(false);
const triggerEl = ref<HTMLElement>();
const menu = ref<HTMLElement>();

/** $attrs（样式类等）落到触发按钮上；aria-label 由 prop 统一接管 */
const triggerAttrs = computed(() => {
    const { class: _class, 'aria-label': _label, ...others } = attrs;
    return others;
});

function positionMenu() {
    const triggerRect = triggerEl.value?.getBoundingClientRect();
    const el = menu.value;
    if (!triggerRect || !el) return;
    const menuWidth = el.offsetWidth;
    const menuHeight = el.offsetHeight;
    let left = triggerRect.left;
    if (left + menuWidth > window.innerWidth - 8) {
        left = Math.max(8, triggerRect.right - menuWidth);
    }
    const wantTop = props.placement.startsWith('top');
    let top = wantTop ? triggerRect.top - menuHeight - 6 : triggerRect.bottom + 6;
    if (!wantTop && top + menuHeight > window.innerHeight - 8) {
        top = Math.max(8, triggerRect.top - menuHeight - 6);
    }
    if (wantTop && top < 8) top = Math.min(window.innerHeight - menuHeight - 8, triggerRect.bottom + 6);
    el.style.left = `${Math.max(8, left)}px`;
    el.style.top = `${top}px`;
}

function toggle() {
    if (props.disabled) return;
    open.value = !open.value;
    if (open.value) nextTick(positionMenu);
}
function hide() {
    open.value = false;
}
function onDocPointerDown(event: PointerEvent) {
    if (!open.value) return;
    const target = event.target as Node;
    if (triggerEl.value?.contains(target) || menu.value?.contains(target)) return;
    open.value = false;
}
function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && open.value) {
        event.stopPropagation();
        open.value = false;
    }
}
onMounted(() => {
    document.addEventListener('pointerdown', onDocPointerDown, true);
    window.addEventListener('keydown', onKeydown);
});
onBeforeUnmount(() => {
    document.removeEventListener('pointerdown', onDocPointerDown, true);
    window.removeEventListener('keydown', onKeydown);
});
defineExpose({ show: () => ((open.value = true), nextTick(positionMenu)), hide });
</script>

<template>
    <button
        ref="triggerEl"
        :class="['lab-popout-trigger', attrs.class]"
        :aria-label="ariaLabel ?? (attrs['aria-label'] as string | undefined)"
        :aria-expanded="open"
        :disabled="disabled"
        v-bind="triggerAttrs"
        @click="toggle"
    >
        <slot />
    </button>
    <Teleport :to="(typeof container === 'object' && container) || 'body'">
        <div v-if="open" ref="menu" class="lab-popout-menu menu-surface" :class="dropdownClass">
            <slot name="menu" :hide="hide" />
        </div>
    </Teleport>
</template>
