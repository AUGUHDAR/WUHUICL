<script setup lang="ts">
/** 我的房间：连接进度、地址、模组同步、日志、退房。 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { call, phaseLabel, refresh, stop, vl } from './useVoxlink';
import { copyText } from '../../../api';
import { toast } from '../../../store';
import './voxlink.css';

const tick = ref(Date.now());
const timer = window.setInterval(() => (tick.value = Date.now()), 1000);
onBeforeUnmount(() => window.clearInterval(timer));

const roomCode = computed(() => String(vl.room?.code || vl.room?.room?.code || ''));
const roomName = computed(() => String(vl.room?.name || '未命名房间'));
const isHost = computed(() => vl.state === 'hosting' || Boolean(vl.room?.isHost));
const address = computed(() => (vl.connection?.status === 'success' ? vl.connection.address || '' : ''));
const elapsed = computed(() => {
    if (!vl.alive) return '';
    const sec = Math.max(0, Math.floor((tick.value - Number(vl.startedAt || 0)) / 1000));
    return sec < 60 ? `${sec} 秒` : `${Math.floor(sec / 60)} 分 ${sec % 60} 秒`;
});
const statusPill = computed(() => {
    if (!vl.alive) return { text: '没有进行中的房间', cls: '' };
    if (address.value) return { text: '已连上', cls: 'ok' };
    if (vl.connection?.status === 'failed') return { text: '这条线没通', cls: 'bad' };
    return { text: '正在连接', cls: 'busy' };
});

const showLogs = ref(false);
const gate = ref<Record<string, unknown> | null>(null);
const gateBusy = ref(false);
const gateError = ref('');
const downloading = ref(false);

async function checkGate() {
    if (!roomCode.value) return;
    gateBusy.value = true;
    gateError.value = '';
    const r = await call<Record<string, unknown>>('vlcli:modsync', { action: 'gate', code: roomCode.value });
    if (!r.ok) gateError.value = r.message || '模组清单没取到';
    else gate.value = r.data || null;
    gateBusy.value = false;
}

async function downloadMods() {
    if (!roomCode.value) return;
    downloading.value = true;
    gateError.value = '';
    const r = await call<Record<string, unknown>>('vlcli:modsync', { action: 'download', code: roomCode.value });
    downloading.value = false;
    if (!r.ok) gateError.value = r.message || '下载没完成';
    else await checkGate();
}

async function rebuildHostList() {
    gateError.value = '';
    const r = await call('vlcli:modsync', { action: 'rebuild' });
    if (!r.ok) gateError.value = r.message || '清单重建失败';
}

async function copy(text: string, label: string) {
    toast((await copyText(text)) ? label : '复制失败，手动选中复制吧', 'info');
}

onMounted(() => {
    void refresh();
});

const missing = computed(() => Number(gate.value?.missing ?? (gate.value?.rows as unknown[])?.length ?? 0));
</script>

<template>
    <div data-ui="voxlink:room">
        <div v-if="!vl.alive" class="vl-card">
            <div class="vl-empty">
                现在没有进行中的房间。去大厅挑一个，或者自己开一个把房号发给朋友。
            </div>
        </div>

        <template v-else>
            <div class="vl-card">
                <div class="vl-card-head">
                    <div>
                        <h2>{{ roomName }}</h2>
                        <p>{{ isHost ? '你是房主，朋友连进来后在你这边进游戏' : '你连进了别人的房间' }}</p>
                    </div>
                    <span class="vl-pill" :class="statusPill.cls">{{ statusPill.text }}</span>
                </div>

                <div class="vl-row">
                    <span class="vl-code">{{ roomCode || '————' }}</span>
                    <button v-if="roomCode" class="btn btn-ghost btn-sm" @click="copy(roomCode, '房号已复制')">复制房号</button>
                    <span class="vl-hint">已用 {{ elapsed }}</span>
                </div>

                <div v-if="address" class="vl-addr" style="margin-top: 12px">
                    <code>{{ address }}</code>
                    <button class="btn btn-sm" @click="copy(address, '服务器地址已复制')">复制地址</button>
                </div>

                <ol v-if="address" class="vl-steps" style="margin-top: 12px">
                    <li>进游戏，多人游戏，添加服务器。</li>
                    <li>地址栏粘贴上面这串，端口已经带在里面了。</li>
                    <li>连不上就回来看下面的日志，多半是模组没对齐。</li>
                </ol>

                <p v-else class="vl-hint" style="margin-top: 12px">
                    {{ phaseLabel(vl.connection?.phase) }} · {{ vl.connection?.detail || '还在试，最长会走到中继' }}
                </p>

                <div v-if="vl.lastError" class="vl-error">{{ vl.lastError }}</div>
                <div v-if="vl.roomMayLinger && !vl.alive" class="vl-notice">
                    刚才那个房间会在服务端自动过期，几分钟内可能还能在大厅里看到。
                </div>
            </div>

            <div v-if="!isHost" class="vl-card">
                <div class="vl-card-head">
                    <div>
                        <h2>必装模组</h2>
                        <p>房主要求的模组，缺了进不去。检查只读清单，下载才会写你的 mods 目录。</p>
                    </div>
                    <div class="vl-row">
                        <button class="btn btn-ghost btn-sm" :disabled="gateBusy || downloading" @click="checkGate">
                            {{ gateBusy ? '检查中…' : '检查差异' }}
                        </button>
                        <button
                            class="btn btn-sm"
                            :disabled="downloading || gateBusy || !missing"
                            @click="downloadMods"
                        >
                            {{ downloading ? '下载中…' : '下载缺失的' }}
                        </button>
                    </div>
                </div>
                <p v-if="gate" class="vl-hint">
                    清单里 {{ (gate.rows as unknown[])?.length ?? 0 }} 项，还缺 {{ missing }} 项。
                </p>
                <div v-if="gateError" class="vl-error">{{ gateError }}</div>
            </div>

            <div v-else class="vl-card">
                <div class="vl-card-head">
                    <div>
                        <h2>房主清单</h2>
                        <p>加了模组之后重建一次，访客拿到的清单才是新的。</p>
                    </div>
                    <button class="btn btn-ghost btn-sm" @click="rebuildHostList">重建清单</button>
                </div>
                <div v-if="gateError" class="vl-error">{{ gateError }}</div>
            </div>

            <div class="vl-card">
                <div class="vl-card-head">
                    <div><h2>连接日志</h2></div>
                    <button class="btn btn-ghost btn-sm" @click="showLogs = !showLogs">
                        {{ showLogs ? '收起' : '展开' }}
                    </button>
                </div>
                <div v-if="showLogs" class="vl-log">
                    <div v-for="(line, i) in vl.logs" :key="i">{{ line.text }}</div>
                    <div v-if="!vl.logs.length" class="vl-hint">还没有日志。</div>
                </div>
            </div>

            <div class="vl-actions">
                <button class="btn btn-danger" @click="stop()">退出房间</button>
            </div>
        </template>
    </div>
</template>
