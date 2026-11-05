<script setup lang="ts">
/** @modrinth/ui EmptyState 兼容实现（type 仅影响插画，此处以图标占位；#actions 放操作按钮）。 */
defineProps<{ type?: string; heading?: string; description?: string }>();
</script>

<template>
    <div class="lab-empty">
        <div class="lab-empty-icon" aria-hidden="true">
            <slot name="icon">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M21 8 12 3 3 8v8l9 5 9-5Z" />
                    <path d="m3 8 9 5 9-5" />
                    <path d="M12 13v8" />
                </svg>
            </slot>
        </div>
        <p v-if="heading" class="lab-empty-heading">{{ heading }}</p>
        <p v-if="description" class="lab-empty-description">{{ description }}</p>
        <slot />
        <div v-if="$slots.actions" class="lab-empty-actions">
            <slot name="actions" />
        </div>
    </div>
</template>
