<script setup lang="ts">
/** 错误状态（UX"失败处理"）：原因 + 具体处理办法 + 重试入口，不直出错误码/堆栈。
 *  调用方负责把内部错误翻译成人话传入 message；hint 给下一步建议。 */
defineProps<{
    message: string;
    hint?: string;
    retryText?: string;
}>();
defineEmits<{ (e: 'retry'): void }>();
</script>

<template>
    <div class="error-state" role="alert">
        <p class="error-state-message">{{ message }}</p>
        <p v-if="hint" class="error-state-hint">{{ hint }}</p>
        <button class="btn btn-ghost btn-sm" @click="$emit('retry')">
            {{ retryText ?? '重试' }}
        </button>
    </div>
</template>

<style scoped>
.error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-6) var(--space-5);
    text-align: center;
    background: var(--danger-soft);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
}
.error-state-message {
    font-size: var(--text-md);
    color: var(--text);
    line-height: 1.7;
    overflow-wrap: anywhere;
}
.error-state-hint {
    font-size: var(--text-sm);
    color: var(--text-dim);
    line-height: 1.7;
    max-width: 460px;
}
</style>