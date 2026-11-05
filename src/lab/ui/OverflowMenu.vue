<script setup lang="ts">
/**
 * @modrinth/ui OverflowMenu 兼容实现：图标触发 + 菜单项列表（icon/label/action/disabled/color）。
 */
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { OverflowMenuOption } from './overflow-menu-option';

withDefaults(
    defineProps<{
        options: OverflowMenuOption[];
        disabled?: boolean;
        tooltip?: string;
        placement?: string;
        ariaLabel?: string;
    }>(),
    { disabled: false, tooltip: undefined, placement: 'bottom-end', ariaLabel: undefined }
);

const open = ref(false);
const triggerEl = ref<HTMLElement>();
const menu = ref<HTMLElement>();

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
    let top = triggerRect.bottom + 6;
    if (top + menuHeight > window.innerHeight - 8) {
        top = Math.max(8, triggerRect.top - menuHeight - 6);
    }
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
}
function toggle() {
    open.value = !open.value;
    if (open.value) nextTick(positionMenu);
}
function pick(action?: () => void) {
    open.value = false;
    action?.();
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
defineExpose({ open: () => (open.value = true), close: () => (open.value = false) });
</script>

<template>
    <button
        ref="triggerEl"
        class="btn btn-ghost btn-sm lab-overflow-trigger"
        :title="tooltip"
        :aria-label="ariaLabel"
        :aria-expanded="open"
        :disabled="disabled"
        v-bind="$attrs"
        @click="toggle"
    >
        <slot>
            <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" aria-hidden="true">
                <circle cx="5" cy="12" r="1.8" />
                <circle cx="12" cy="12" r="1.8" />
                <circle cx="19" cy="12" r="1.8" />
            </svg>
        </slot>
    </button>
    <Teleport to="body">
        <div v-if="open" ref="menu" class="lab-popout-menu menu-surface lab-overflow-menu">
            <template v-for="option in options" :key="option.id">
                <hr v-if="option.divider" class="lab-menu-divider" />
                <button
                    v-else
                    type="button"
                    class="lab-menu-item"
                    :class="{ disabled: option.disabled, danger: option.color === 'red' }"
                    :disabled="option.disabled"
                    :title="option.label"
                    @click="pick(option.action)"
                >
                    <component :is="option.icon" v-if="option.icon" aria-hidden="true" />
                    <span v-if="option.label">{{ option.label }}</span>
                </button>
            </template>
        </div>
    </Teleport>
</template>
