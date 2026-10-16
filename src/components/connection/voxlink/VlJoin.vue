<script setup lang="ts">
/** 加入房间：房号、密码、必装模组门控。 */
import { onMounted, ref } from 'vue';
import { call, join, pendingJoinCode, refresh, section, vl } from './useVoxlink';
import './voxlink.css';

const code = ref('');
const password = ref('');
const skipGate = ref(false);
const busy = ref(false);
const error = ref('');
const preview = ref<Record<string, unknown> | null>(null);

onMounted(() => {
    if (pendingJoinCode.value) {
        code.value = pendingJoinCode.value;
        pendingJoinCode.value = '';
        void lookup();
    }
});

function normalize(raw: string): string {
    const cleaned = raw.replace(/[^0-9a-zA-Z]/g, '').toUpperCase();
    return cleaned.slice(0, 6);
}

async function lookup() {
    error.value = '';
    preview.value = null;
    const c = normalize(code.value);
    if (c.length !== 6) return;
    const r = await call<Record<string, unknown>>('vlcli:roomInfo', { code: c });
    if (!r.ok) {
        error.value = r.message || '没找到这个房间';
        return;
    }
    preview.value = (r.data || {}) as Record<string, unknown>;
}

async function submit() {
    const c = normalize(code.value);
    if (c.length !== 6) {
        error.value = '房号是 6 位，检查一下有没有少';
        return;
    }
    code.value = c;
    busy.value = true;
    error.value = '';
    const r = await join(c, password.value, skipGate.value ? 'skip' : 'gate');
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '加入失败';
        if (r.data) preview.value = r.data as Record<string, unknown>;
        return;
    }
    section.value = 'room';
    await refresh();
}
</script>

<template>
    <div class="vl-card" data-ui="voxlink:join">
        <div class="vl-card-head">
            <div>
                <h2>加入房间</h2>
                <p>输入朋友发给你的 6 位房号。房主开了密码就一起填上。</p>
            </div>
        </div>

        <label class="vl-field">
            <span>房号</span>
            <input
                v-model="code"
                class="input"
                maxlength="6"
                placeholder="例如 AB12CD"
                :disabled="busy"
                @input="code = normalize(code)"
                @blur="lookup"
            />
        </label>

        <label class="vl-field">
            <span>房间密码（没有就留空）</span>
            <input v-model="password" class="input" type="password" :disabled="busy" />
        </label>

        <label class="vl-row" style="margin-bottom: 12px">
            <input id="vl-skip-gate" v-model="skipGate" type="checkbox" :disabled="busy" />
            <span class="vl-hint">跳过必装模组检查，直接进房</span>
        </label>

        <div v-if="preview" class="vl-notice">
            <strong>{{ preview.name || '未命名房间' }}</strong>
            · {{ preview.gameVersion || '版本未知' }}
            <template v-if="preview.loader"> · {{ preview.loader }}</template>
            · {{ preview.currentPlayers ?? '?' }}/{{ preview.maxPlayers ?? '?' }} 人
        </div>

        <div v-if="error" class="vl-error">{{ error }}</div>

        <div class="vl-actions">
            <button class="btn" :disabled="busy" @click="submit">
                {{ busy ? '正在加入…' : '加入房间' }}
            </button>
            <button class="btn btn-ghost" :disabled="busy" @click="section = 'lobby'">去大厅看看</button>
        </div>

        <p v-if="vl.lastError" class="vl-error">{{ vl.lastError }}</p>
    </div>
</template>
