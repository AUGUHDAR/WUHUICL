<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, nextTick, computed } from 'vue'
const props = defineProps<{ text: string }>()
const outer = ref<HTMLElement>(), inner = ref<HTMLElement>(), distance = ref(0)
let observer: ResizeObserver | undefined
const measure = () => { distance.value = Math.max(0, (inner.value?.scrollWidth ?? 0) - (outer.value?.clientWidth ?? 0)) }
onMounted(() => { observer = new ResizeObserver(measure); if (outer.value) observer.observe(outer.value); measure() })
onUnmounted(() => observer?.disconnect())
watch(() => props.text, async () => { await nextTick(); measure() })
const style = computed(() => ({ '--travel': `-${distance.value}px`, '--duration': `${Math.max(5, distance.value / 32 + 3)}s` }))
</script>
<template><span ref="outer" class="marquee" :class="{ overflow: distance > 1 }" :style="style" :title="text" tabindex="0"><span ref="inner">{{ text }}</span></span></template>
<style scoped>
.marquee { display: block; min-width: 0; max-width: 100%; overflow: hidden; white-space: nowrap; }
.marquee > span { display: inline-block; }
.overflow > span { animation: readable-scroll var(--duration) ease-in-out infinite; }
.marquee:hover > span, .marquee:focus > span { animation-play-state: running; }
@keyframes readable-scroll { 0%,18%,100% { transform: translateX(0); } 65%,82% { transform: translateX(var(--travel)); } }
@media (prefers-reduced-motion: reduce) { .overflow > span { animation: none; } .marquee { overflow-x: auto; } }
</style>
