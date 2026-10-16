<script setup lang="ts">
/** 设置：联机组件状态、连接偏好、本机在听的端口。 */
import { onMounted, ref } from 'vue';
import { vl, vlCall as call, vlEnsure as ensure } from './useVoxlink';
import './voxlink.css';

type Row = Record<string, unknown>;

const theme = ref('light');
const tag = ref('');
const relay = ref(false);
const ports = ref<unknown[]>([]);
const saving = ref(false);
const checking = ref(false);
const compError = ref('');
const prefError = ref('');
const portError = ref('');

// 端口结果同样由 CLI 透传，键名不固定，试完几种常见形状再退回空数组
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
    const parts = [o.port ?? o.localPort ?? o.id, o.protocol, o.address];
    return parts.map((v) => String(v ?? '').trim()).filter(Boolean).join(' ') || '未知端口';
}

async function loadSettings(): Promise<void> {
    const r = await call<Row>('vlcli:settingsGet');
    if (!r.ok) { prefError.value = r.message || '设置没读回来'; return; }
    const d = r.data || {};
    theme.value = String(d.theme || 'light');
    tag.value = String(d.clientTag || '');
    relay.value = d.relayEnabled === true;
}

async function loadPorts(): Promise<void> {
    portError.value = '';
    const r = await call('vlcli:netPorts');
    if (r.ok) ports.value = asList(r.data);
    else portError.value = r.message || '端口没读回来';
}

async function recheck(): Promise<void> {
    checking.value = true;
    compError.value = '';
    const ok = await ensure(true);
    checking.value = false;
    if (!ok) compError.value = vl.prepareError || '联机组件没准备好';
}

async function save(): Promise<void> {
    const value = tag.value.trim();
    if (value.length > 10) { prefError.value = '客户端标签最多 10 个字，先改短'; return; }
    saving.value = true;
    prefError.value = '';
    const r = await call('vlcli:settingsSave', {
        theme: theme.value,
        clientTag: value,
        relayEnabled: relay.value
    });
    saving.value = false;
    if (!r.ok) prefError.value = r.message || '设置没保存';
    else await loadSettings();
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
                <div><h2>联机组件</h2><p>大厅、建房、工单都靠它。</p></div>
                <button class="btn btn-ghost btn-sm" :disabled="checking" @click="recheck">检查更新</button>
            </div>
            <div class="vl-row">
                <span class="vl-pill" :class="vl.ready ? 'ok' : 'bad'">版本 {{ vl.version || '未知' }}</span>
                <span class="vl-pill">协议 {{ vl.protocol || '未知' }}</span>
                <span v-for="c in vl.capabilities" :key="c" class="vl-pill">{{ c }}</span>
            </div>
            <div v-if="compError" class="vl-error">{{ compError }}</div>
        </div>

        <div class="vl-card">
            <div class="vl-card-head"><h2>连接偏好</h2></div>
            <label class="vl-field">
                <span>客户端标签</span>
                <input class="input" v-model="tag" :disabled="saving" placeholder="最多 10 个字" />
            </label>
            <label class="vl-field">
                <span>主题</span>
                <select class="input" v-model="theme" :disabled="saving">
                    <option value="light">浅色</option>
                    <option value="dark">深色</option>
                </select>
            </label>
            <label class="vl-row">
                <input v-model="relay" type="checkbox" :disabled="saving" /> 直连不通时走玩家中继
            </label>
            <div v-if="prefError" class="vl-error">{{ prefError }}</div>
            <div class="vl-actions"><button class="btn" :disabled="saving" @click="save">保存</button></div>
        </div>

        <div class="vl-card">
            <div class="vl-card-head">
                <div><h2>本机端口</h2><p>建房时填游戏正在听的那个。</p></div>
                <button class="btn btn-ghost btn-sm" @click="loadPorts">刷新</button>
            </div>
            <div v-if="ports.length" class="vl-row">
                <span v-for="(p, i) in ports" :key="i" class="vl-pill ok">{{ portText(p) }}</span>
            </div>
            <div v-else class="vl-empty">没检测到在听的游戏端口</div>
            <div v-if="portError" class="vl-error">{{ portError }}</div>
        </div>
    </div>
</template>
