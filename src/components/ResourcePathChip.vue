<script setup lang="ts">
/**
 * 资源页统一的路径信息条：同一位置（页头正下）、同一背景 UI、文字可选中、
 * 点击复制完整路径。所有按实例/文件夹管理资源的页面一律复用本组件，
 * 不允许再各自造裸文本条（样式/位置/可复制性不一致 = 用户每页重学一套）。
 */
import { toast } from '../store';

const props = defineProps<{
    /** 信息条标签：如「版本文件夹」「导入与收藏保存位置」 */
    label: string;
    /** 完整路径（空 = 尚未读取，占位显示） */
    path: string;
}>();

const COPIED_HINT = '已复制完整路径';
async function copyPath() {
    const text = props.path;
    if (!text) return;
    try {
        await navigator.clipboard.writeText(text);
        toast(COPIED_HINT, 'success');
    } catch {
        // WebView2 剪贴板写入被拒时回退 execCommand（同步、无权限弹窗）
        const input = document.createElement('textarea');
        input.value = text;
        input.style.position = 'fixed';
        input.style.opacity = '0';
        document.body.appendChild(input);
        input.select();
        const ok = document.execCommand('copy');
        input.remove();
        toast(ok ? COPIED_HINT : '复制失败，请手动选择路径文本复制', ok ? 'success' : 'error');
    }
}
</script>

<template>
    <button
        type="button"
        class="rp-chip"
        :title="path ? `${path}（点击复制完整路径）` : undefined"
        :disabled="!path"
        @click="copyPath"
    >
        <span class="rp-chip-label">{{ label }}</span
        ><span class="rp-chip-path">{{ path || '正在读取…' }}</span>
    </button>
</template>

<style scoped>
.rp-chip {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    max-width: 100%;
    background: var(--accent-soft);
    color: var(--text);
    border: 0;
    padding: 6px 12px;
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.6;
    cursor: copy;
    text-align: left;
}
.rp-chip:disabled {
    cursor: default;
}
.rp-chip:hover:not(:disabled) {
    outline: 1px solid var(--accent);
}
.rp-chip-label {
    color: var(--text-dim);
    flex: none;
}
.rp-chip-path {
    min-width: 0;
    overflow-wrap: anywhere;
    user-select: text;
}
</style>
