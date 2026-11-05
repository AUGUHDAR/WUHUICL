<script setup lang="ts">
/**
 * Axolotl components/ui/InstanceIcon.vue 的本地替身：icon-path 走 wuhuicl
 * 受管资产协议；无图标时回退首字母方块。
 */
import { computed } from 'vue';
import { instanceIconUrl } from '../instance';

const props = defineProps<{
    iconPath?: string;
    instanceId?: string;
    loader?: string;
}>();

const src = computed(() => instanceIconUrl(props.iconPath));
const initial = computed(() => (props.instanceId ?? '?').trim().charAt(0).toUpperCase() || '?');
</script>

<template>
    <img v-if="src" class="lab-avatar" :src="src" alt="" loading="lazy" />
    <span v-else class="lab-avatar lab-avatar-fallback">{{ initial }}</span>
</template>
