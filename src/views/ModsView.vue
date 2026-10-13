<script setup lang="ts">
import FileManager from '../components/FileManager.vue'

const icon =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8 12 3 3 8v8l9 5 9-5Z"/><path d="m3 8 9 5 9-5"/><path d="M12 13v8"/></svg>'
</script>

<template>
  <FileManager
    title="模组"
    rel="mods"
    open-label="打开模组文件夹"
    empty-text="暂无模组，将 .jar 文件放入 mods 文件夹"
    :icon="icon"
  />
</template>
