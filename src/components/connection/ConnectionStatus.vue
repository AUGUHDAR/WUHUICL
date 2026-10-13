<script setup lang="ts">
withDefaults(defineProps<{ tone?: 'neutral' | 'success' | 'danger' | 'pending'; label: string }>(), { tone: 'neutral' })
</script>
<template><span class="connection-status" :class="tone"><i aria-hidden="true"></i>{{ label }}</span></template>
