<script setup lang="ts">
/** 微软账号品牌标（四色方块）：HomeView 账号面板与 BrandFaceCard 共用的单一来源。
 *  品牌素材例外（UI_SYSTEM §8）：官方四色方块，非自创图形。 */
defineProps<{ size?: number }>();
</script>
<template>
    <svg
        :width="size ?? 22"
        :height="size ?? 22"
        viewBox="0 0 22 22"
        role="img"
        aria-label="Microsoft 账号"
    >
        <rect x="1" y="1" width="9" height="9" fill="#f25022" />
        <rect x="12" y="1" width="9" height="9" fill="#7fba00" />
        <rect x="1" y="12" width="9" height="9" fill="#00a4ef" />
        <rect x="12" y="12" width="9" height="9" fill="#ffb900" />
    </svg>
</template>
