<script setup lang="ts">
/** VoxLink 联机控制台：六个分区，组件未就绪时只出准备态。 */
import { computed, defineAsyncComponent, onMounted, ref } from 'vue';
import { useVoxlink, vl, section, type VlSection } from './useVoxlink';
import './voxlink.css';

const VlLobby = defineAsyncComponent(() => import('./VlLobby.vue'));
const VlJoin = defineAsyncComponent(() => import('./VlJoin.vue'));
const VlCreate = defineAsyncComponent(() => import('./VlCreate.vue'));
const VlRoom = defineAsyncComponent(() => import('./VlRoom.vue'));
const VlTickets = defineAsyncComponent(() => import('./VlTickets.vue'));
const VlSettings = defineAsyncComponent(() => import('./VlSettings.vue'));

const NAV: Array<{ key: VlSection; label: string }> = [
    { key: 'lobby', label: '大厅' },
    { key: 'join', label: '加入房间' },
    { key: 'create', label: '创建房间' },
    { key: 'room', label: '我的房间' },
    { key: 'tickets', label: '工单' },
    { key: 'settings', label: '设置' }
];

const panels: Record<VlSection, unknown> = {
    lobby: VlLobby,
    join: VlJoin,
    create: VlCreate,
    room: VlRoom,
    tickets: VlTickets,
    settings: VlSettings
};

const currentPanel = computed(() => panels[section.value]);
const inRoom = computed(() => vl.alive && vl.state !== 'idle');
const { ensure } = useVoxlink();

const retrying = ref(false);
async function retry() {
    retrying.value = true;
    await ensure();
    retrying.value = false;
}

onMounted(() => {
    void ensure();
});
</script>

<template>
    <div class="vl-shell" data-ui="voxlink:console">
        <div v-if="!vl.ready" class="vl-prepare">
            <div v-if="vl.preparing" class="vl-spinner" role="status" aria-label="准备中"></div>
            <p v-if="vl.preparing">{{ vl.prepareText || '请稍等…' }}</p>
            <template v-else>
                <p>{{ vl.prepareError || '联机组件暂时不可用' }}</p>
                <button class="btn btn-ghost" :disabled="retrying" @click="retry">
                    {{ retrying ? '正在重试…' : '重试' }}
                </button>
            </template>
        </div>

        <template v-else>
            <nav class="vl-nav" aria-label="联机分区">
                <button
                    v-for="item in NAV"
                    :key="item.key"
                    class="vl-nav-item"
                    :class="{ active: section === item.key }"
                    @click="section = item.key"
                >
                    {{ item.label }}<span v-if="item.key === 'room' && inRoom" class="dot"></span>
                </button>
            </nav>

            <div class="vl-body" :key="section">
                <component :is="currentPanel" />
            </div>
        </template>
    </div>
</template>
