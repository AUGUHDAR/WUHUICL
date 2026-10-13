<script setup lang="ts">
import FileManager from '../components/FileManager.vue'

const icon =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/></svg>'
</script>

<template>
  <FileManager
    title="资源包"
    rel="resourcepacks"
    open-label="打开资源包文件夹"
    empty-text="暂无资源包，将 .zip 文件放入 resourcepacks 文件夹"
    :icon="icon"
  />
</template>
