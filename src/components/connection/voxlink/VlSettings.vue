<script setup lang="ts">
/** 设置：联机组件状态、中继偏好、本机在听的端口。 */
import { onMounted, ref } from 'vue';
import { toast } from '../../../store';
import { vl, call, ensure } from './useVoxlink';
import './voxlink.css';

type Row = Record<string, unknown>;

const relay = ref(false);
const relayBusy = ref(false);
const ports = ref<unknown[]>([]);
const checking = ref(false);
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

async function recheck(): Promise<void> {
    checking.value = true;
    compError.value = '';
    const ok = await ensure(true);
    checking.value = false;
    if (ok) toast('联机组件已是最新', 'success');
    else compError.value = vl.prepareError || '联机组件没准备好';
}

onMounted(() => {
    void loadSettings();
    void loadPorts();
});
</script>

<template>
    <div data-ui="voxlink:settings">
        <div class="vl-card">
            <div class="vl-card-head">
                <div><h2>联机组件</h2><p>大厅、建房、工单都靠它，玩家不用管。</p></div>
                <div class="vl-row">
                    <button class="btn btn-ghost btn-sm" :disabled="checking" @click="recheck">
                        {{ checking ? '检查中…' : '检查更新' }}
                    </button>
                </div>
            </div>
            <div class="vl-row">
                <span class="vl-pill" :class="vl.ready ? 'ok' : 'bad'">版本 {{ vl.version || '未知' }}</span>
                <span class="vl-pill">协议 {{ vl.protocol || '未知' }}</span>
                <span v-for="c in vl.capabilities" :key="c" class="vl-pill">{{ c }}</span>
            </div>
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
