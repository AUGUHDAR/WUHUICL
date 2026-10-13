<script setup lang="ts">
defineProps<{ title: string; subtitle?: string; step?: string }>()
</script>
<template>
  <section class="connection-panel">
    <header class="connection-panel-head">
      <span v-if="step" class="connection-step" aria-hidden="true">{{ step }}</span>
      <div><h2>{{ title }}</h2><p v-if="subtitle">{{ subtitle }}</p></div>
      <div v-if="$slots.action" class="connection-panel-action"><slot name="action" /></div>
    </header>
    <div class="connection-panel-body"><slot /></div>
  </section>
</template>
