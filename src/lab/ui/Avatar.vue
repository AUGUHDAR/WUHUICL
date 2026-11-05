<script setup lang="ts">
/** @modrinth/ui Avatar 兼容实现（世界/实例图标）。 */
defineProps<{ src?: string }>();
</script>

<template>
    <img class="lab-avatar" :src="src" alt="" loading="lazy" />
</template>
