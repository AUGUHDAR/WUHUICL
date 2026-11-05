<script setup lang="ts">
/**
 * @modrinth/ui NewModal 兼容实现：header + 默认插槽 + 可滚动内容区；
 * 暴露 show()/hide()；遮罩点击 / Esc 关闭（closable）。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue';

const props = withDefaults(
    defineProps<{
        header?: string;
        width?: string;
        maxWidth?: string;
        scrollable?: boolean;
        maxContentHeight?: string;
        closable?: boolean;
        hideHeader?: boolean;
        closeOnClickOutside?: boolean;
    }>(),
    {
        header: undefined,
        width: '560px',
        maxWidth: '92vw',
        scrollable: false,
        maxContentHeight: undefined,
        closable: true,
        hideHeader: false,
        closeOnClickOutside: true
    }
);

const open = ref(false);
const visible = ref(false);
let hideTimer: ReturnType<typeof setTimeout> | undefined;

function show() {
    clearTimeout(hideTimer);
    open.value = true;
    requestAnimationFrame(() => (visible.value = true));
}
function hide() {
    if (!open.value) return;
    visible.value = false;
    hideTimer = setTimeout(() => (open.value = false), 140);
}
function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && open.value && props.closable) {
        event.stopPropagation();
        hide();
    }
}
onMounted(() => window.addEventListener('keydown', onKeydown));
onBeforeUnmount(() => {
    window.removeEventListener('keydown', onKeydown);
    clearTimeout(hideTimer);
});
defineExpose({ show, hide });
</script>

<template>
    <Teleport to="body">
        <div v-if="open" class="modal-mask lab-modal-mask" :class="{ shown: visible }" @pointerdown.self="closable && closeOnClickOutside && hide()">
            <div
                class="modal lab-modal"
                role="dialog"
                aria-modal="true"
                :style="{ width, maxWidth }"
            >
                <header v-if="!hideHeader" class="lab-modal-head">
                    <slot name="title">
                        <h3 v-if="header" class="modal-title">{{ header }}</h3>
                    </slot>
                    <button v-if="closable" type="button" class="btn btn-ghost btn-sm lab-btn-icon" aria-label="关闭" @click="hide">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12" /></svg>
                    </button>
                </header>
                <div
                    class="lab-modal-body"
                    :class="{ scrollable }"
                    :style="maxContentHeight ? { maxHeight: maxContentHeight } : undefined"
                >
                    <slot />
                </div>
            </div>
        </div>
    </Teleport>
</template>
