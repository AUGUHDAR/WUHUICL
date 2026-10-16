<script setup lang="ts">
/** 工单：列问题、填附件路径、看回复并追问。 */
import { computed, onMounted, ref } from 'vue';
import { vl, vlCall as call } from './useVoxlink';
import './voxlink.css';

type Row = Record<string, unknown>;
type View = 'list' | 'create' | 'detail';

const view = ref<View>('list');
const tickets = ref<Row[]>([]);
const detail = ref<Row | null>(null);
const text = ref('');
const draft = ref('');
const files = ref<string[]>([]);
const busy = ref(false);
const error = ref('');
const TITLES: Record<View, string> = { list: '我的工单', create: '新建工单', detail: '工单详情' };

const str = (v: unknown) => (v === undefined || v === null ? '' : String(v));
const stamp = (v: unknown) => (Number(v) ? new Date(Number(v)).toLocaleString() : '时间未知');
const isCreate = computed(() => view.value === 'create');
const msgs = computed<Row[]>(() => (detail.value ? rows(detail.value.messages) : []));

// 后端只透传 CLI 输出，列表字段名不固定，几种常见形状各试一次再退回空数组
function rows(data: unknown): Row[] {
    if (Array.isArray(data)) return data as Row[];
    const obj = (data || {}) as Row;
    for (const key of ['items', 'tickets', 'list']) {
        if (Array.isArray(obj[key])) return obj[key] as Row[];
    }
    return [];
}

function state(t: Row): { text: string; cls: string } {
    if (t.deleted === true) return { text: '已删除', cls: 'bad' };
    if (t.hasUnread === true) return { text: '有回复', cls: 'ok' };
    if (Number(t.replyCount) > 0) return { text: '已回复', cls: 'busy' };
    return { text: '等待回复', cls: '' };
}

function fail(msg?: string): void {
    error.value = msg || vl.lastError || '工单操作没完成';
}

function show(next: View): void {
    view.value = next;
    if (next !== 'detail') detail.value = null;
    text.value = ''; files.value = []; error.value = '';
}

async function load(): Promise<void> {
    busy.value = true;
    const r = await call('vlcli:tickets', { action: 'list' });
    busy.value = false;
    if (r.ok) tickets.value = rows(r.data);
    else fail(r.message);
}

async function open(id: string): Promise<void> {
    busy.value = true;
    const r = await call<Row>('vlcli:tickets', { action: 'detail', id });
    busy.value = false;
    if (!r.ok) { fail(r.message); return; }
    detail.value = r.data || {};
    show('detail');
    void call('vlcli:tickets', { action: 'viewed', id });
}

function addFile(): void {
    const p = draft.value.trim();
    draft.value = '';
    if (p && files.value.length < 10 && !files.value.includes(p)) files.value.push(p);
}

async function send(action: 'submit' | 'reply'): Promise<void> {
    const body = text.value.trim();
    const id = str(detail.value?.id);
    if (!body) { error.value = isCreate.value ? '先把问题写清楚' : '先写要追问的内容'; return; }
    const payload = action === 'submit'
        ? { action, description: body, files: files.value }
        : { action, id, text: body, files: files.value };
    busy.value = true;
    error.value = '';
    const r = await call('vlcli:tickets', payload);
    busy.value = false;
    if (!r.ok) { fail(r.message); return; }
    if (action === 'submit') {
        show('list');
        await load();
        return;
    }
    await open(id);
    void load();
}

onMounted(() => void load());
</script>

<template>
    <div class="vl-card" data-ui="voxlink:tickets">
        <div class="vl-card-head">
            <div><h2>{{ TITLES[view] }}</h2><p>问题和附件发给客服，回复后这里会更新。</p></div>
            <button v-if="view === 'list'" class="btn btn-ghost btn-sm" @click="load">刷新</button>
            <button v-if="view === 'list'" class="btn btn-sm" @click="show('create')">新建工单</button>
            <button v-if="view !== 'list'" class="btn btn-ghost btn-sm" @click="show('list')">返回</button>
        </div>
        <p v-if="error" class="vl-error" role="alert">{{ error }}</p>

        <template v-if="view === 'list'">
            <div v-if="busy" class="vl-spinner" role="status" aria-label="加载中"></div>
            <div v-else-if="tickets.length" class="vl-grid">
                <div v-for="t in tickets" :key="str(t.id)">
                    <article class="vl-card">
                        <div class="vl-row">
                            <strong>#{{ str(t.id) }}</strong>
                            <span class="vl-pill" :class="state(t).cls">{{ state(t).text }}</span>
                        </div>
                        <p class="vl-hint">{{ str(t.description) || '这条工单没写描述' }}</p>
                        <p class="vl-players">{{ stamp(t.lastTimeMs || t.timeMs) }}</p>
                        <button class="btn btn-sm" @click="open(str(t.id))">查看</button>
                    </article>
                </div>
            </div>
            <div v-else class="vl-empty">还没有工单</div>
        </template>

        <template v-else>
            <p v-if="detail" class="vl-code">#{{ str(detail.id) }}</p>
            <p v-if="detail" class="vl-players">{{ stamp(detail.timeMs) }}</p>
            <p v-if="detail" class="vl-hint">{{ str(detail.description) || '这条工单没写描述' }}</p>
            <article v-for="(m, i) in msgs" :key="i" class="vl-card">
                <strong>{{ str(m.from) === 'admin' ? 'VoxLink 回复' : '我的追问' }}</strong>
                <p class="vl-players">{{ stamp(m.timeMs) }}</p>
                <p class="vl-hint">{{ str(m.text) }}</p>
            </article>
            <label class="vl-field">
                <span>{{ isCreate ? '问题描述' : '追问内容' }}</span>
                <textarea class="input" rows="5" v-model="text" :disabled="busy"
                    :placeholder="isCreate ? '发生了什么、怎么复现' : '补充信息，或回答客服'"></textarea>
            </label>
            <div class="vl-field">
                <span>附件路径</span>
                <div class="vl-row">
                    <input class="input" v-model="draft" placeholder="本机绝对路径" @keyup.enter="addFile" />
                    <button class="btn btn-ghost btn-sm" :disabled="busy" @click="addFile">添加</button>
                </div>
            </div>
            <ul v-if="files.length" class="vl-steps">
                <li v-for="(f, i) in files" :key="f">{{ f }}
                    <button class="btn btn-ghost btn-sm" @click="files.splice(i, 1)">移除</button>
                </li>
            </ul>
            <p class="vl-hint">附件填本机绝对路径，一条消息最多 10 个，提交时才上传。</p>
            <div class="vl-actions">
                <button class="btn" :disabled="busy" @click="send(isCreate ? 'submit' : 'reply')">
                    {{ isCreate ? '提交' : '追问' }}
                </button>
            </div>
        </template>
    </div>
</template>
