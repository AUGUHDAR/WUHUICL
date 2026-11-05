<script setup lang="ts">
/** VoxLink 联机控制台：进页零阻塞（就绪直接用，没就绪显示后台获取进度），六个分区。 */
import { computed, defineAsyncComponent, onMounted, ref, watch } from 'vue';
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
    await ensure(true);
    retrying.value = false;
}

/** 组件还没就绪时（首次获取）展示后台流水线进度；下载可以离开页面继续。 */
const prepareLine = computed(() => {
    const u = vl.update;
    switch (u.phase) {
        case 'checking':
            return '正在确认最新版本…';
        case 'downloading':
            return u.progress !== null && u.progress !== undefined
                ? `正在获取联机组件… ${u.progress}%`
                : `正在获取联机组件… ${Math.round((u.bytes || 0) / 1024)} KB`;
        case 'pending':
            return '新版本已就绪，联机空闲时自动替换。';
        case 'failed':
            return u.message || '这次没连上更新源，稍后会自动重试。';
        default:
            return '正在获取联机组件…';
    }
});
const prepareFailed = computed(() => vl.update.phase === 'failed');

// 后台首次获取完成时，把就绪态捞回来（用户可能一直停在这页）
watch(
    () => vl.update.phase,
    (p) => {
        if ((p === 'latest' || p === 'pending') && !vl.ready) void ensure();
    }
);

onMounted(() => {
    void ensure();
});
</script>

<template>
    <div class="vl-shell" data-ui="voxlink:console">
        <div v-if="!vl.ready" class="vl-prepare">
            <div class="vl-spinner" role="status" aria-label="准备中"></div>
            <p>{{ vl.prepareError || prepareLine }}</p>
            <p v-if="!vl.prepareError && !prepareFailed" class="vl-hint">
                下载在后台进行，可以先去别的页面逛逛，回来就好了。
            </p>
            <button
                v-if="vl.prepareError || prepareFailed"
                class="btn btn-ghost"
                :disabled="retrying"
                @click="retry"
            >
                {{ retrying ? '正在重试…' : '重试' }}
            </button>
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
