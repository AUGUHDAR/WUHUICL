<script setup lang="ts">
/** 设置：联机组件版本与后台更新状态、中继偏好、本机在听的端口。 */
import { computed, onMounted, ref } from 'vue';
import { toast } from '../../../store';
import { vl, call, ensure, vlCheckUpdate } from './useVoxlink';
import './voxlink.css';

type Row = Record<string, unknown>;

const relay = ref(false);
const relayBusy = ref(false);
const ports = ref<unknown[]>([]);
const compError = ref('');
const prefError = ref('');
const portError = ref('');

// 端口由组件透传，键名不固定，试完几种常见形状再退回空数组
function asList(data: unknown): unknown[] {
    if (Array.isArray(data)) return data;
    const obj = (data || {}) as Row;
    for (const key of ['items', 'ports', 'list']) {
        if (Array.isArray(obj[key])) return obj[key] as unknown[];
    }
    return [];
}

function portText(p: unknown): string {
    if (p === null || typeof p !== 'object') return String(p);
    const o = p as Row;
    return [o.port ?? o.localPort ?? o.id, o.protocol, o.address]
        .map((v) => String(v ?? '').trim())
        .filter(Boolean)
        .join(' ') || '未知端口';
}

async function loadSettings(): Promise<void> {
    prefError.value = '';
    const r = await call<Row>('vlcli:settingsGet');
    if (!r.ok) {
        prefError.value = r.message || '设置没读回来';
        return;
    }
    relay.value = (r.data || {}).relayEnabled === true;
}

async function loadPorts(): Promise<void> {
    portError.value = '';
    const r = await call('vlcli:netPorts');
    if (r.ok) ports.value = asList(r.data);
    else portError.value = r.message || '端口没读回来';
}

async function toggleRelay(): Promise<void> {
    relayBusy.value = true;
    prefError.value = '';
    const r = await call('vlcli:settingsSave', { relayEnabled: relay.value });
    relayBusy.value = false;
    if (!r.ok) {
        relay.value = !relay.value;
        prefError.value = r.message || '这个开关没保存上';
        return;
    }
    toast(relay.value ? '允许走玩家中继' : '只走直连，不通就失败', 'success');
}

/** 检查更新不阻塞：点一下就丢给后台，结果反映在下面的状态行里。 */
async function checkNow(): Promise<void> {
    compError.value = '';
    await vlCheckUpdate();
}

const updateBusy = computed(() => vl.update.phase === 'checking' || vl.update.phase === 'downloading');

const updateLine = computed(() => {
    const u = vl.update;
    switch (u.phase) {
        case 'checking':
            return '正在检查更新…';
        case 'downloading':
            return u.progress !== null && u.progress !== undefined
                ? `正在后台更新到 ${u.latest || '新版本'}（${u.progress}%），不影响现在的使用。`
                : `正在后台更新到 ${u.latest || '新版本'}，不影响现在的使用。`;
        case 'pending':
            return `新版本 ${u.latest || ''} 已就绪，联机空闲时自动替换。`;
        case 'failed':
            return `更新源暂时连不上，稍后会自动重试。${u.message ? `（${u.message}）` : ''}`;
        case 'latest':
            return u.latest && u.latest !== vl.version
                ? `已取到最新版 ${u.latest}，正在就位。`
                : '已是最新版本。更新检查在后台自动进行。';
        default:
            return '更新检查在后台自动进行，有新版本会静默下载、空闲时替换。';
    }
});

onMounted(() => {
    void loadSettings();
    void loadPorts();
    void ensure();
});
</script>

<template>
    <div data-ui="voxlink:settings">
        <div class="vl-card">
            <div class="vl-card-head">
                <div><h2>联机组件</h2><p>大厅、建房、工单都靠它，玩家不用管。</p></div>
                <div class="vl-row">
                    <button class="btn btn-ghost btn-sm" :disabled="updateBusy" @click="checkNow">
                        {{ updateBusy ? (vl.update.phase === 'downloading' ? '更新中…' : '检查中…') : '检查更新' }}
                    </button>
                </div>
            </div>
            <div class="vl-row">
                <span class="vl-pill" :class="vl.ready ? 'ok' : 'bad'">版本 {{ vl.version || '未知' }}</span>
                <span class="vl-pill">协议 {{ vl.protocol || '未知' }}</span>
                <span v-for="c in vl.capabilities" :key="c" class="vl-pill">{{ c }}</span>
            </div>
            <p class="vl-hint" style="margin-top: 10px">{{ updateLine }}</p>
            <div v-if="compError" class="vl-error">{{ compError }}</div>
        </div>

        <div class="vl-card">
            <div class="vl-card-head">
                <div><h2>连接偏好</h2><p>直连打洞走不通时，允不允许转到玩家中继。</p></div>
            </div>
            <label class="vl-row">
                <input v-model="relay" type="checkbox" :disabled="relayBusy" @change="toggleRelay" />
                <span class="vl-hint">允许走玩家中继</span>
            </label>
            <div v-if="prefError" class="vl-error">{{ prefError }}</div>
        </div>

        <div class="vl-card">
            <div class="vl-card-head">
                <div><h2>本机端口</h2><p>游戏开着并对外局域网开放时，这里会列出在听的端口。</p></div>
                <div class="vl-row">
                    <button class="btn btn-ghost btn-sm" @click="loadPorts">刷新</button>
                </div>
            </div>
            <div v-if="ports.length" class="vl-row">
                <span v-for="(p, i) in ports" :key="i" class="vl-pill ok">{{ portText(p) }}</span>
            </div>
            <div v-else class="vl-empty">没检测到在听的游戏端口</div>
            <div v-if="portError" class="vl-error">{{ portError }}</div>
        </div>
    </div>
</template>
