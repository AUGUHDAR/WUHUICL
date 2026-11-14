<script setup lang="ts">
/**
 * 全站统一模态原语（UI_SYSTEM §5 弹层五类之一）。
 * 复用全局 .modal-mask/.modal/.modal-actions 样式；焦点陷阱与初始焦点由全局
 * modalFocus.ts 托管；无「取消」类按钮的弹窗由 app:modal-esc 兜底事件关闭
 * （modalFocus.ts 在 ESC 找不到 dismiss 按钮时派发，见该文件）。
 *
 * 分工口径（UX「打断要分级」）：模态只留给破坏性操作与必须当场做的决定；
 * 轻量选择用 SelectMenu，情境动作用 AppMenu，普通成败用 toast。
 */
import { onBeforeUnmount, ref, watch } from 'vue';

const props = withDefaults(
    defineProps<{
        open: boolean;
        /** 弹窗标题：渲染 .modal-title，同时作为 aria-label */
        title?: string;
        /** 内容宽度档位；不传 = 全局 .modal 默认 460px */
        size?: 'sm' | 'md' | 'lg' | 'xl';
        /** true = 提交进行中：遮罩/ESC 不关闭（防误触丢进度） */
        busy?: boolean;
        /** false = 点遮罩不关闭（必须显式选择去向的弹窗用） */
        dismissable?: boolean;
    }>(),
    { busy: false, dismissable: true }
);
const emit = defineEmits<{ (e: 'close'): void }>();

const panel = ref<HTMLElement | null>(null);

function tryDismiss() {
    if (props.dismissable && !props.busy) emit('close');
}
function onEscFallback() {
    if (props.open && !props.busy) emit('close');
}

// 面板随 open 挂载/卸载，兜底监听跟着走
let bound: HTMLElement | null = null;
watch(
    panel,
    (el, prev) => {
        prev?.removeEventListener('app:modal-esc', onEscFallback);
        bound = null;
        if (el) {
            el.addEventListener('app:modal-esc', onEscFallback);
            bound = el;
        }
    },
    { flush: 'post' }
);
onBeforeUnmount(() => bound?.removeEventListener('app:modal-esc', onEscFallback));
</script>

<template>
    <Teleport to="body">
        <div v-if="open" class="modal-mask" @pointerdown.self="tryDismiss">
            <div
                ref="panel"
                class="modal"
                :class="size ? `modal--${size}` : ''"
                role="dialog"
                aria-modal="true"
                :aria-label="title || '对话框'"
            >
                <h3 v-if="title" class="modal-title">{{ title }}</h3>
                <slot />
                <div v-if="$slots.actions" class="modal-actions">
                    <slot name="actions" />
                </div>
            </div>
        </div>
    </Teleport>
</template>
